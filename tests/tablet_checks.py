#!/usr/bin/env python3
"""Opt-in physical tablet smoke test, with optional backed-up APK installation."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time
import urllib.parse
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = 'com.notemaker'


def archive_hashes(path):
    with tarfile.open(path) as archive:
        return {m.name: hashlib.sha256(archive.extractfile(m).read()).hexdigest()
                for m in archive if m.isfile()}


def verify_history(before, after):
    """Catch missing or altered actions, wrong roots, and altered actor histories."""
    old = {op['id']: op for op in before['events'] + before['pending']}
    new = {op['id']: op for op in after['events'] + after['pending']}
    if any(new.get(identity) != op for identity, op in old.items()):
        raise AssertionError('Pre-upgrade actions are missing or changed')
    if before.get('serverId') and before['serverId'] != after.get('serverId'):
        raise AssertionError('Main server identity changed')
    if before.get('actors', {}) != after.get('actors', {}):
        raise AssertionError('Actor histories changed; keep the tablet idle during this check')
    return len(old)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--install', action='store_true', help='Stop, back up, and upgrade while retaining data')
    parser.add_argument('--apk', type=Path, default=ROOT / 'apps/android/build/notemaker.apk')
    parser.add_argument('--reverse-port', type=int, help='Leave this explicit USB server mapping active')
    parser.add_argument('--timeout', type=float, default=60)
    args = parser.parse_args()
    if args.reverse_port is not None and not 1 <= args.reverse_port <= 65535:
        parser.error('Invalid reverse port')
    if args.serial.startswith('emulator-'):
        parser.error('Use the emulator tier for emulators')
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    devices = subprocess.check_output(['adb', 'devices'], text=True, timeout=15)
    if not any(line.split()[:2] == [args.serial, 'device'] for line in devices.splitlines()):
        print('SKIP: selected tablet is absent or not authorized: ' + args.serial)
        return 77
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / '.test-artifacts/tablet' / stamp
    out.mkdir(parents=True, mode=0o700)
    report = dict(serial=args.serial, install=args.install, status='running')
    adb = ['adb', '-s', args.serial]
    forwarded = None
    stopped = False
    def call(*arguments, timeout=30):
        return subprocess.check_output(adb + list(arguments), timeout=timeout)
    def save(name, *arguments):
        with (out / name).open('wb') as target:
            subprocess.run(adb + list(arguments), stdout=target, check=True, timeout=60)
    def launch():
        call('shell', 'am', 'start', '-n', PACKAGE + '/.MainActivity')
    try:
        prefs = ET.fromstring(call('shell', 'run-as', PACKAGE, 'cat', 'shared_prefs/settings.xml'))
        settings = {child.attrib['name']: child.text or '' for child in prefs}
        if settings.get('proxyUrl'):
            raise ValueError('This tablet check requires its embedded proxy; use the network tier for an external selection')
        document = settings.get('document', 'notes')
        report['document'] = document
        apk_path = call('shell', 'pm', 'path', PACKAGE).decode().strip().removeprefix('package:')
        if args.install:
            signer = Path(os.environ['ANDROID_HOME']) / 'build-tools/36.0.0/apksigner'
            subprocess.run([str(signer), 'verify', str(args.apk)], check=True, timeout=30)
            call('shell', 'am', 'force-stop', PACKAGE)
            stopped = True
            save('before.tar', 'exec-out', 'run-as', PACKAGE, 'tar', '-cf', '-', 'files', 'shared_prefs')
            call('pull', apk_path, str(out / 'before.apk'), timeout=60)
            # Extract the trusted, freshly collected archive with safe paths only.
            with tarfile.open(out / 'before.tar') as archive:
                archive.extractall(out / 'before', filter='data')
            before = json.loads(subprocess.check_output([
                str(ROOT / 'apps/cli/notemaker'), '--state', str(out / 'before/files'),
                '--document', document, 'log'], timeout=30))
            call('install', '--no-incremental', '-r', str(args.apk), timeout=120)
            save('after-install.tar', 'exec-out', 'run-as', PACKAGE, 'tar', '-cf', '-', 'files', 'shared_prefs')
            original = archive_hashes(out / 'before.tar')
            if original != archive_hashes(out / 'after-install.tar'):
                raise AssertionError('App files/preferences changed during installation')
            report['preserved_files'] = len(original)
            apk_path = call('shell', 'pm', 'path', PACKAGE).decode().strip().removeprefix('package:')
            save('installed.apk', 'exec-out', 'cat', apk_path)
            expected = hashlib.sha256(args.apk.read_bytes()).hexdigest()
            if hashlib.sha256((out / 'installed.apk').read_bytes()).hexdigest() != expected:
                raise AssertionError('Installed APK does not match the selected build')
            report['apk_sha256'] = expected
        if args.reverse_port:
            call('reverse', f'tcp:{args.reverse_port}', f'tcp:{args.reverse_port}')
        call('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        call('shell', 'wm', 'dismiss-keyguard')
        launch()
        stopped = False
        # Use the JVM CLI over an ADB forward: some BOOX versions stall ART's
        # standalone app_process shutdown even after the command has completed.
        deadline = time.monotonic() + args.timeout
        last_error = None
        while time.monotonic() < deadline:
            try:
                descriptor = json.loads(call('shell', 'run-as', PACKAGE, 'cat', 'files/proxy.json'))
                port = urllib.parse.urlparse(descriptor['url']).port
                if not port:
                    raise ValueError('Proxy descriptor has no port')
                forwarded = call('forward', 'tcp:0', f'tcp:{port}').decode().strip()
                descriptor['url'] = 'http://127.0.0.1:' + forwarded
                endpoint = out / 'forwarded-proxy.json'
                endpoint.write_text(json.dumps(descriptor))
                endpoint.chmod(0o600)
                break
            except (subprocess.SubprocessError, ValueError, KeyError) as error:
                last_error = error
                time.sleep(1)
        else:
            raise AssertionError(f'Proxy startup timed out: {last_error}')
        def cli(command):
            return json.loads(subprocess.check_output([
                str(ROOT / 'apps/cli/notemaker'), '--proxy-file', str(endpoint),
                '--document', document, command], timeout=15))
        while time.monotonic() < deadline:
            status = cli('status')
            report['proxy'] = status
            if status.get('connected') and status['pending'] == 0:
                break
            time.sleep(1)
        else:
            raise AssertionError('Proxy did not connect and drain: ' + str(report.get('proxy')))
        after = cli('log')
        if args.install:
            report['preserved_actions'] = verify_history(before, after)
        pid = call('shell', 'pidof', PACKAGE).decode().strip()
        logs = call('logcat', '-d', '--pid=' + pid).decode(errors='replace')
        (out / 'logcat.txt').write_text(logs)
        if 'FATAL EXCEPTION' in logs:
            raise AssertionError('Fatal exception in app log')
        save('tablet.png', 'exec-out', 'screencap', '-p')
        report['status'] = 'pass'
    except Exception as error:
        report.update(status='fail', error=str(error))
    finally:
        # Retain diagnostics on failures too; a disconnected device must not
        # hide the original error or prevent the summary being written.
        for name, arguments in [
            ('failure-logcat.txt', ('logcat', '-d', '-s', 'NoteMaker', 'AndroidRuntime')),
            ('failure.png', ('exec-out', 'screencap', '-p')),
        ] if report['status'] == 'fail' else []:
            try:
                save(name, *arguments)
            except Exception as error:
                report.setdefault('diagnostic_errors', []).append(str(error))
        if forwarded:
            try:
                call('forward', '--remove', 'tcp:' + forwarded)
            except Exception as error:
                report.update(status='fail', cleanup_error=str(error))
        if stopped:
            try:
                launch()
            except Exception:
                pass
        (out / 'report.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    print('Artifacts: ' + str(out))
    return int(report['status'] != 'pass')


if __name__ == '__main__':
    sys.exit(main())

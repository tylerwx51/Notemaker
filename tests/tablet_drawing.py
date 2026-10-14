#!/usr/bin/env python3
"""Opt-in BOOX drawing checks in a new notebook; restore the prior selection."""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time
import urllib.parse
import xml.etree.ElementTree as ET
from tablet_checks import verify_history

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = 'com.notemaker'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    args = parser.parse_args()
    if not os.environ.get('IN_NIX_SHELL') or args.serial.startswith('emulator-'):
        parser.error('Run through nix-shell with an explicit physical device serial')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    out = ROOT / '.test-artifacts/tablet-drawing' / stamp
    out.mkdir(parents=True, mode=0o700)
    report = dict(serial=args.serial, document='Drawing-check-' + stamp, status='running', checks=[])
    forwarded = None
    changed = False

    def adb(*parts):
        result = subprocess.run(['adb', '-s', args.serial, *parts], capture_output=True, timeout=90)
        if result.returncode:
            raise RuntimeError(result.stderr.decode(errors='replace'))
        return result.stdout

    def instrument(action, **values):
        parts = ['am', 'instrument', '-w', '-e', 'action', action]
        for key, value in values.items():
            parts += ['-e', key, str(value)]
        text = adb('shell', shlex.join(parts + [PACKAGE + '/.Integration'])).decode()
        if 'PASS ' + action not in text:
            raise AssertionError('Instrumentation failed: ' + text)
        adb('shell', 'am', 'start', '-f', '0x20000000', '-n', PACKAGE + '/.MainActivity')

    def tree():
        adb('shell', 'uiautomator', 'dump', '/data/local/tmp/notemaker-drawing-check.xml')
        return ET.fromstring(adb('shell', 'cat', '/data/local/tmp/notemaker-drawing-check.xml'))

    def bounds(value):
        for node in tree().iter('node'):
            if value in (node.get('text'), node.get('content-desc')):
                return list(map(int, re.findall(r'\d+', node.get('bounds'))))
        raise AssertionError('Missing UI control: ' + value)

    def tap(value):
        l, t, r, b = bounds(value)
        adb('shell', 'input', 'tap', str((l+r)//2), str((t+b)//2))
        time.sleep(.4)

    def gesture():
        l, t, r, b = bounds('Drawing canvas')
        adb('shell', 'input', 'swipe', str(l+(r-l)//6), str(t+(b-t)//4),
            str(l+(r-l)//3), str(t+(b-t)//3), '450')

    def cli(document, *parts, value=None):
        nonlocal forwarded
        # Instrumentation may restart the app and its ephemeral proxy port.
        # Wait for a responding proxy, not merely a descriptor left on disk.
        # Only the read-only readiness probe is retried, never a mutation.
        deadline = time.monotonic()+30
        while True:
            descriptor = json.loads(adb('shell', 'run-as', PACKAGE, 'cat', 'files/proxy.json'))
            port = urllib.parse.urlparse(descriptor['url']).port
            if forwarded:
                adb('forward', '--remove', 'tcp:'+forwarded)
            forwarded = adb('forward', 'tcp:0', f'tcp:{port}').decode().strip()
            descriptor['url'] = 'http://127.0.0.1:' + forwarded
            endpoint.write_text(json.dumps(descriptor)); endpoint.chmod(0o600)
            probe = subprocess.run([str(ROOT/'apps/cli/notemaker'), '--proxy-file', str(endpoint),
                                    '--document', document, 'status'], capture_output=True, text=True, timeout=10)
            if probe.returncode==0:
                break
            if time.monotonic()>=deadline:
                raise AssertionError('Proxy readiness timed out: '+probe.stderr)
            time.sleep(.5)
        result = subprocess.run([str(ROOT/'apps/cli/notemaker'), '--proxy-file', str(endpoint),
                                 '--document', document, '--actor', 'hardware-check', *parts],
                                input=None if value is None else json.dumps(value),
                                capture_output=True, text=True, timeout=30)
        if result.returncode:
            raise AssertionError(result.stderr)
        return json.loads(result.stdout)

    def check(name):
        report['checks'].append(name)
        print('PASS ' + name, flush=True)

    try:
        prefs = ET.fromstring(adb('shell', 'run-as', PACKAGE, 'cat', 'shared_prefs/settings.xml'))
        original = {e.get('name'): e.text or '' for e in prefs}
        if original.get('proxyUrl'):
            raise AssertionError('Select the embedded proxy before running this hardware check')
        adb('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        adb('shell', 'wm', 'dismiss-keyguard')
        adb('shell', 'am', 'start', '-n', PACKAGE + '/.MainActivity')
        descriptor = json.loads(adb('shell', 'run-as', PACKAGE, 'cat', 'files/proxy.json'))
        port = urllib.parse.urlparse(descriptor['url']).port
        forwarded = adb('forward', 'tcp:0', f'tcp:{port}').decode().strip()
        descriptor['url'] = 'http://127.0.0.1:' + forwarded
        endpoint = out/'proxy.json'
        endpoint.write_text(json.dumps(descriptor)); endpoint.chmod(0o600)
        old_document = original.get('document', 'notes')
        before = cli(old_document, 'log')
        (out/'original-before.json').write_text(json.dumps(before))
        old_type = cli(old_document, 'inspect')['type']
        original_proxy = json.loads(adb('shell', 'run-as', PACKAGE, 'cat', 'files/proxy-config.json'))
        # Idle proxies can retain their last successful connectivity status.
        # Probe the existing edge without changing it or creating an action.
        try:
            cli(old_document, 'sync')
        except AssertionError:
            pass
        if cli(old_document, 'status').get('connected'):
            raise AssertionError('This offline check requires an already unavailable upstream; no connection settings were changed')
        document = report['document']
        changed = True
        instrument('configure', document=document, host=original.get('host',''), type='fixed',
                   proxyUrl=original.get('proxyUrl',''), proxyToken=original.get('proxyToken',''))
        tap('Shape tools')
        bounds('Create Triangle')
        (out/'palette.png').write_bytes(adb('exec-out', 'screencap', '-p'))
        tap('Create Triangle'); gesture(); time.sleep(1)
        model = cli(document, 'inspect')
        assert len(model['objects']) == 1 and model['objects'][0]['shape'] == 'triangle', model
        check('BOOX toolbar and shape drag persisted through embedded proxy')
        tap('Text placement'); tap('Enter text…'); tap('Text to place')
        adb('shell', 'input', 'text', 'BOOX-test-text')
        tap('Continue'); gesture(); time.sleep(1)
        model = cli(document, 'inspect')
        text = model['objects'][-1]
        assert text['shape'] == 'text' and text['text'] == 'BOOX-test-text'
        check('BOOX text dialog and canvas placement')
        instrument('shape', shape='star')
        star = cli(document, 'inspect')['objects'][-1]
        instrument('undo')
        assert cli(document, 'inspect')['objectActive'][star['object']] is False
        instrument('redo')
        assert cli(document, 'inspect')['objectActive'][star['object']] is True
        instrument('shape', shape='line', cancel='true')
        assert len(cli(document, 'inspect')['objects']) == 3
        check('Atomic creation undo/redo and canceled gesture')
        cli(document, 'object', 'update', text['object'], '-', value={'text':'CLI-edited-BOOX-text'})
        time.sleep(1)
        instrument('state')
        state = json.loads(adb('shell', 'run-as', PACKAGE, 'cat', 'files/integration.json'))
        assert next(o for o in state['model']['objects'] if o['object']==text['object'])['text']=='CLI-edited-BOOX-text'
        saved = cli(document, 'log')
        assert len([op for op in saved['pending'] if op['kind']=='batch']) == 6
        assert cli(document, 'status')['pending'] > 0
        session = 'boox-hardware-selection'
        entity = dict(kind='object', id=text['object'], page=0)
        cli(document, 'selection', 'select', '-', value=dict(session=session, page=0, entities=[entity]))
        transform = dict(session=session, entities=[entity], transform=dict(dx=25, scale=1.25))
        count = len(saved['pending'])
        cli(document, 'transform', 'preview', '-', value=transform)
        assert len(cli(document, 'log')['pending']) == count
        assert next(o for o in cli(document, 'inspect')['objects'] if o['object']==text['object'])['x'] == text['x']
        cli(document, 'transform', 'cancel', session)
        cli(document, 'transform', 'commit', '-', value=transform)
        assert len(cli(document, 'log')['pending']) == count+1
        resized = next(o for o in cli(document, 'inspect')['objects'] if o['object']==text['object'])
        assert abs(resized['fontSize']-text['fontSize']*1.25) < .001
        check('BOOX proxy previews stay transient and text transform saves one atomic action')
        copied = cli(document, 'selection', 'copy', session)
        cli(document, 'selection', 'deselect', session)
        cli(document, 'selection', 'paste', '-', value=dict(copied, dx=30, dy=30))
        assert len(cli(document, 'inspect')['objects']) == 4
        cli(document, 'fill', '-', value=dict(object=star['object'], color=-65536))
        cli(document, 'template', 'add', '-', value=dict(template='grid', page=0))
        cli(document, 'stroke', 'put', '-', value=dict(stroke='hardware-erase', layer='base', page=0,
            width=2, color=-16777216, points=[[100,700,1],[300,700,1]]))
        cli(document, 'erase', '-', value=dict(mode='partial', points=[[200,680],[200,720]], radius=9))
        model = cli(document, 'inspect')
        assert model['strokeActive']['hardware-erase'] is False
        assert len([s for s in model['strokes'] if s['stroke']!='hardware-erase']) == 2
        check('BOOX clipboard, native fill, background template and atomic partial eraser')
        instrument('draw', shift=100, color=-16776961)
        assert any(len(s['points']) >= 12 for s in cli(document, 'inspect')['strokes'])
        check('BOOX pressure-bearing gesture retains its final stroke samples')
        saved = cli(document, 'log')
        (out/'drawing.png').write_bytes(adb('exec-out', 'screencap', '-p'))
        adb('shell', 'am', 'force-stop', PACKAGE)
        instrument('state')
        verify_history(saved, cli(document, 'log'))
        check('CLI-to-Android updates and offline history survive process restart')
        report['status'] = 'pass'
    except Exception as error:
        report.update(status='fail', error=str(error))
    finally:
        if changed:
            try:
                instrument('configure', document=old_document, host=original.get('host',''),
                           type=old_type, proxyUrl=original.get('proxyUrl',''),
                           proxyToken=original.get('proxyToken',''))
                report['preserved_original_actions'] = verify_history(before, cli(old_document, 'log'))
                restored = ET.fromstring(adb('shell', 'run-as', PACKAGE, 'cat', 'shared_prefs/settings.xml'))
                settings = {e.get('name'): e.text or '' for e in restored}
                assert all(settings.get(k,'')==original.get(k,'') for k in ('document','host','proxyUrl','proxyToken'))
                proxy = json.loads(adb('shell', 'run-as', PACKAGE, 'cat', 'files/proxy-config.json'))
                assert proxy.get('upstream')==original_proxy.get('upstream'), 'Effective upstream configuration changed'
                assert proxy.get('rootId')==original_proxy.get('rootId'), 'Proxy root identity changed'
                report['restored_selection'] = True
                report['preserved_upstream'] = True
            except Exception as error:
                report.update(status='fail', restore_error=str(error))
        try:
            (out/'logcat.txt').write_bytes(adb('logcat', '-d', '-s', 'NoteMaker', 'AndroidRuntime'))
            if report['status']=='fail':
                (out/'failure.png').write_bytes(adb('exec-out', 'screencap', '-p'))
        except Exception as error:
            report.setdefault('diagnostic_errors', []).append(str(error))
        if forwarded:
            try:
                adb('forward', '--remove', 'tcp:'+forwarded)
            except Exception as error:
                report.update(status='fail', cleanup_error=str(error))
        (out/'report.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    print('Artifacts: ' + str(out))
    return int(report['status']!='pass')


if __name__ == '__main__':
    raise SystemExit(main())

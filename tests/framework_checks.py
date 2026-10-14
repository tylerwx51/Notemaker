#!/usr/bin/env python3
"""Regression checks for test selection, preservation gates, and network harness."""
import json
import contextlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tarfile
import unittest
from unittest.mock import patch
import tablet_checks
import network_checks
from run import plan, execute
from tablet_checks import verify_history
from network_checks import check
from topology_support import Proxy, eventually
import server_checks


class FrameworkChecks(unittest.TestCase):
    def test_default_is_independent_of_devices(self):
        selected = plan(['local'], None)
        self.assertTrue(selected)
        self.assertFalse(any(c['name'] in ('tablet_checks', 'network_checks') for c in selected))
        self.assertTrue(all(c['skip'] is None for c in selected))

    def test_external_resources_are_explicit(self):
        self.assertTrue(all(c['skip'] for c in plan(['hardware', 'network'], None)))
        self.assertTrue(all(c['skip'] is None for c in plan(['hardware', 'network'], None,
                                                         serial='selected', topology='selected.json')))
        with self.assertRaises(ValueError):
            plan(['local'], ['tablet_checks'])

    def test_failed_subprocess_is_recordable(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / 'failure.log'
            self.assertEqual(execute([sys.executable, '-c', 'print("failure fixture"); exit(3)'], log), 3)
            self.assertIn('failure fixture', log.read_text())

    def test_skip_can_be_required(self):
        command = [sys.executable, str(Path(__file__).with_name('run.py')), '--tier', 'hardware', '--no-build']
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
        self.assertEqual(subprocess.run(command + ['--require-resources'], capture_output=True).returncode, 1)

    def test_preservation_accepts_commits_but_rejects_loss_or_changes(self):
        op = dict(id='retained', kind='layer', name='Original')
        before = dict(events=[], pending=[op], serverId='root', actors={'human': {'undo': ['a']}})
        after = dict(before, events=[op], pending=[])
        self.assertEqual(verify_history(before, after), 1)
        for damaged in [dict(after, events=[]), dict(after, events=[dict(op, name='Changed')]),
                        dict(after, serverId='other'), dict(after, actors={})]:
            with self.assertRaises(AssertionError):
                verify_history(before, damaged)

    def test_wrong_network_root_is_rejected_before_any_edits(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            for name in ('a', 'b'):
                (directory / (name + '.json')).write_text(json.dumps(dict(url='http://' + name, token='fixture')))
            topology = directory / 'topology.json'
            topology.write_text(json.dumps(dict(server='http://root', proxies=['a.json', 'b.json'])))
            peers = [dict(kind='server', protocol=3, id='root'),
                     dict(kind='proxy', protocol=3, id='a', rootId='root'),
                     dict(kind='proxy', protocol=3, id='b', rootId='other')]
            with patch.object(network_checks, 'http', side_effect=peers), \
                 patch('network_checks.subprocess.check_output') as cli:
                with self.assertRaisesRegex(AssertionError, 'selected main server'):
                    check(topology, directory)
                cli.assert_not_called()

    def test_upgrade_workflow_and_preservation_failures_without_hardware(self):
        # A strict fake ADB exercises backup/install/verification ordering while
        # ensuring local tests can never accidentally touch a real device.
        for fault in (None, 'data', 'apk'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                apk = root / 'new.apk'
                apk.write_bytes(b'verified build')
                trace = []
                state = dict(events=[dict(id='retained', kind='layer', name='Original')],
                             pending=[], actors={}, serverId='root')
                def output(command, **kwargs):
                    trace.append(command)
                    if command == ['adb', 'devices']:
                        return 'List of devices attached\nfixture\tdevice\n'
                    if command[0] != 'adb':
                        if command[-1] == 'status':
                            return json.dumps(dict(connected=True, pending=0)).encode()
                        if command[-1] == 'log':
                            return json.dumps(state).encode()
                        raise AssertionError(command)
                    args = command[3:]
                    if args[:2] == ['shell', 'run-as'] and args[-1] == 'shared_prefs/settings.xml':
                        return b'<map><string name="document">notes</string></map>'
                    if args[:3] == ['shell', 'pm', 'path']:
                        return b'package:/installed.apk\n'
                    if args[0] == 'pull':
                        Path(args[-1]).write_bytes(b'old apk')
                        return b''
                    if args[0] == 'install':
                        backups = list((root / '.test-artifacts/tablet').glob('*/before.tar'))
                        self.assertEqual(len(backups), 1)
                        self.assertTrue(backups[0].with_name('before.apk').exists())
                        return b'Success'
                    if args[-1] == 'files/proxy.json':
                        return b'{"url":"http://127.0.0.1:8766","token":"fixture"}'
                    if args[:2] == ['forward', 'tcp:0']:
                        return b'12345\n'
                    if args[:2] == ['shell', 'pidof']:
                        return b'123\n'
                    if args[0] == 'logcat' or args[:2] == ['forward', '--remove'] or args[:2] in (
                            ['shell', 'am'], ['shell', 'input'], ['shell', 'wm']):
                        return b''
                    raise AssertionError(command)
                def run(command, **kwargs):
                    trace.append(command)
                    if command[0] != 'adb':
                        self.assertEqual(command[-2:], ['verify', str(apk)])
                        return subprocess.CompletedProcess(command, 0)
                    args = command[3:]
                    target = kwargs['stdout']
                    if 'tar' in args:
                        data = b'preserved' if fault != 'data' or not target.name.endswith('after-install.tar') else b'changed'
                        with tarfile.open(fileobj=target, mode='w') as archive:
                            member = tarfile.TarInfo('files/document.json')
                            member.size = len(data)
                            archive.addfile(member, io.BytesIO(data))
                    elif args == ['exec-out', 'cat', '/installed.apk']:
                        target.write(b'wrong build' if fault == 'apk' else apk.read_bytes())
                    elif args == ['exec-out', 'screencap', '-p'] or args[0] == 'logcat':
                        target.write(b'fixture')
                    else:
                        raise AssertionError(command)
                    return subprocess.CompletedProcess(command, 0)
                with patch.object(tablet_checks, 'ROOT', root), \
                     patch.dict(os.environ, IN_NIX_SHELL='1', ANDROID_HOME=temp), \
                     patch.object(sys, 'argv', ['tablet_checks', '--serial', 'fixture', '--install', '--apk', str(apk)]), \
                     patch('tablet_checks.subprocess.check_output', side_effect=output), \
                     patch('tablet_checks.subprocess.run', side_effect=run), \
                     contextlib.redirect_stdout(io.StringIO()):
                    code = tablet_checks.main()
                report = json.loads(next((root / '.test-artifacts/tablet').glob('*/report.json')).read_text())
                self.assertEqual(code, int(fault is not None), report)
                self.assertEqual(report['status'], 'pass' if fault is None else 'fail')
                if fault is None:
                    self.assertEqual(report['preserved_actions'], 1)
                    self.assertTrue(any(c[-3:] == ['forward', '--remove', 'tcp:12345'] for c in trace))
                else:
                    self.assertTrue(any(c[-4:] == ['am', 'start', '-n', 'com.notemaker/.MainActivity'] for c in trace))


class NetworkHarnessChecks(server_checks.ServerChecks):
    def test_same_harness_against_real_local_proxy_chain(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            parent = Proxy(directory / 'parent', upstream=self.url)
            child = None
            try:
                eventually(lambda: parent.command('status')['serverId'])
                child = Proxy(directory / 'child', upstream=parent.descriptor_path)
                eventually(lambda: child.command('status')['serverId'])
                topology = directory / 'topology.json'
                topology.write_text(json.dumps(dict(server=self.url, proxies=[
                    str(parent.descriptor_path), str(child.descriptor_path)])))
                report = check(topology, directory)
                self.assertEqual(report['status'], 'pass')
                self.assertEqual(report['committed_actions'], 2)
            finally:
                if child:
                    child.stop()
                parent.stop()


if __name__ == '__main__':
    unittest.main(verbosity=2)

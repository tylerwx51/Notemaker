#!/usr/bin/env python3
"""Real HTTP chat lifecycle, streaming, restart recovery and Bubblewrap checks."""
import concurrent.futures
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'apps/server'))
from agent.harness import PiHarness


class ChatChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.data = Path(self.temp.name)
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            self.port = probe.getsockname()[1]
        self.url = f'http://127.0.0.1:{self.port}'
        self.start()

    def start(self):
        self.process = subprocess.Popen([sys.executable, str(ROOT / 'apps/server/server.py'),
            '--host', '127.0.0.1', '--port', str(self.port), '--database', str(self.data / 'chat.sqlite3')],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            try:
                self.get('/api/chats/options')
                return
            except OSError:
                time.sleep(.05)
        raise RuntimeError('Server did not start')

    def tearDown(self):
        self.process.terminate()
        self.process.wait(timeout=10)
        self.temp.cleanup()

    def get(self, path):
        with urllib.request.urlopen(self.url + path, timeout=10) as response:
            return json.load(response)

    def post(self, path, data):
        req = urllib.request.Request(self.url + path, json.dumps(data).encode(), {'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=10) as response:
            return json.load(response)

    def create(self, harness='mock'):
        body = self.get('/api/chats/options')['defaults'] | {'id': str(uuid.uuid4()), 'harness': harness}
        chat = self.post('/api/chats', body)
        self.assertEqual(self.post('/api/chats', body)['id'], chat['id'])
        return chat['id']

    def send(self, chat, text='hello', request=None):
        body = {'request': request or str(uuid.uuid4()), 'text': text, 'document': 'chat-check'}
        self.post(f'/api/chats/{chat}/messages', body)
        return body

    def wait_done(self, chat, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            state = self.get('/api/chats/' + chat)
            if state['status'] != 'running':
                return state
            time.sleep(.05)
        self.fail('Run did not finish')

    def test_streaming_reconnect_and_idempotency(self):
        chat = self.create()
        body = self.send(chat, 'stream ' * 30)
        with urllib.request.urlopen(self.url + f'/api/chats/{chat}/stream', timeout=10) as response:
            observed = []
            for line in response:
                row = json.loads(line)
                if row:
                    observed.append(row)
                    if row['event']['kind'] == 'delta':
                        break
            self.assertEqual(self.get('/api/chats/' + chat)['status'], 'running')
        self.post(f'/api/chats/{chat}/messages', body)
        with self.assertRaises(urllib.error.HTTPError) as conflict:
            self.post(f'/api/chats/{chat}/messages', body | {'text': 'different'})
        self.assertEqual(conflict.exception.code, 409)
        self.assertEqual(self.wait_done(chat)['status'], 'done')
        tail = self.get(f'/api/chats/{chat}/events?after={observed[-1]["seq"]}')['events']
        all_events = self.get(f'/api/chats/{chat}/events')['events']
        self.assertEqual(observed + tail, all_events)
        self.assertEqual(sum(row['event']['kind'] == 'user' for row in all_events), 1)
        self.assertIn('tool_end', [row['event']['kind'] for row in all_events])
        deltas = ''.join(row['event']['text'] for row in all_events if row['event']['kind'] == 'delta')
        final = next(row['event']['text'] for row in all_events if row['event']['kind'] == 'assistant_end')
        self.assertEqual(deltas, final)
        self.send(chat, 'second turn')
        self.assertEqual(self.wait_done(chat)['status'], 'done')

    def test_history_pagination_and_restart(self):
        chats = [self.create() for _ in range(7)]
        recent = self.get('/api/chats')
        self.assertEqual([chat['id'] for chat in recent['chats']], chats[::-1][:5])
        self.assertTrue(recent['more'])
        more = self.get('/api/chats?offset=5')
        self.assertEqual(len(more['chats']), 2)
        self.send(chats[0], 'long-running ' * 100)
        self.process.kill(); self.process.wait(timeout=10)
        self.start()
        self.assertEqual(self.get('/api/chats/' + chats[0])['status'], 'interrupted')
        self.assertEqual(self.get('/api/chats')['chats'][0]['id'], chats[0])
        events = self.get('/api/chats/' + chats[0] + '/events')['events']
        self.assertEqual(events[-1]['event']['status'], 'interrupted')

    def test_stop_and_concurrent_send(self):
        chat = self.create()
        body = {'request': 'same', 'text': 'slow ' * 100, 'document': 'chat-check'}
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            results = list(pool.map(lambda _: self.post(f'/api/chats/{chat}/messages', body), range(3)))
        self.assertEqual(results, [{'accepted': 'same'}] * 3)
        with self.assertRaises(urllib.error.HTTPError) as conflict:
            self.send(chat)
        self.assertEqual(conflict.exception.code, 409)
        self.post(f'/api/chats/{chat}/stop', {})
        self.assertEqual(self.wait_done(chat)['status'], 'stopped')
        self.send(chat)
        self.assertEqual(self.wait_done(chat)['status'], 'done')

    def test_validation(self):
        with self.assertRaises(urllib.error.HTTPError):
            self.post('/api/chats', {'id': '../escape'})
        with self.assertRaises(urllib.error.HTTPError) as missing:
            self.get('/api/chats/missing')
        self.assertEqual(missing.exception.code, 404)
        with self.assertRaises(urllib.error.HTTPError):
            self.get('/api/chats?limit=-1')
        req = urllib.request.Request(self.url + '/api/chats', b'{}',
              {'Content-Type': 'application/json', 'Origin': 'https://example.com'})
        with self.assertRaises(urllib.error.HTTPError) as origin:
            urllib.request.urlopen(req)
        self.assertEqual(origin.exception.code, 403)

    def test_pi_stop_during_startup(self):
        chat = self.create('pi')
        self.send(chat, 'Reply briefly.')
        start = time.monotonic()
        self.post(f'/api/chats/{chat}/stop', {})
        self.assertEqual(self.wait_done(chat)['status'], 'stopped')
        self.assertLess(time.monotonic() - start, 6)

    def test_sandbox_cli_and_network(self):
        harness = PiHarness(ROOT, self.data / 'sandbox', 'pi', self.url)
        self.addCleanup(harness.close)
        chat = {'id': str(uuid.uuid4()), 'provider': 'openai', 'model': 'gpt-6-luna', 'effort': 'medium'}
        command = harness.command(chat)
        command = command[:command.index(shutil.which('node'))]
        # This file is outside the repository and outside the sandbox data mount.
        secret = self.data / 'outside.txt'; secret.write_text('must not be visible')
        script = '''import json, pathlib, subprocess, urllib.request
assert not pathlib.Path(%r).exists()
assert not pathlib.Path('/home/tyler/.ssh').exists()
p=pathlib.Path(%r); p.write_text('writable'); p.unlink()
print(json.load(urllib.request.urlopen(%r))['protocol'])
args=['apps/cli/notemaker','--state',%r,'--document','sandbox-check','--actor','agent-check']
for tail in [['document','create','fixed'], ['layer','put','-'], ['sync',%r], ['inspect']]:
 result=subprocess.run(args+tail,input=json.dumps({'layer':'agent-layer','name':'Agent layer','force':False}),text=True,capture_output=True)
 assert result.returncode==0, result.stderr
 print(result.stdout)
''' % (str(secret), str(ROOT / '.test-artifacts/sandbox-write'), self.url + '/api/info', str(harness.data / 'replica'), self.url)
        (ROOT / '.test-artifacts').mkdir(exist_ok=True)
        result = subprocess.run(command + [sys.executable, '-c', script], text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = self.get('/api/events?document=sandbox-check')['events']
        self.assertEqual(len(rows), 2)
        self.assertIn('Agent layer', result.stdout)

    def test_reuse_agent_proxy_after_server_restart(self):
        first=PiHarness(ROOT,self.data/'adopt','pi',self.url)
        self.addCleanup(first.close)
        first.ensure_proxy()
        descriptor=json.loads((first.data/'replica/proxy.json').read_text())
        resumed=PiHarness(ROOT,first.data,'pi',self.url)
        resumed.ensure_proxy()
        self.assertIsNone(resumed.proxy)
        self.assertEqual(json.loads((first.data/'replica/proxy.json').read_text()),descriptor)


if __name__ == '__main__':
    unittest.main()

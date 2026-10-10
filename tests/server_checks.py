#!/usr/bin/env python3
"""Protocol checks against the actual HTTP server and its SQLite transaction."""
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]

class ServerChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        cls.url = f'http://127.0.0.1:{port}'
        cls.process = subprocess.Popen([sys.executable, str(ROOT / 'apps/server/server.py'), '--host', '127.0.0.1', '--port', str(port), '--database', cls.temp.name + '/test.sqlite3'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(50):
            try:
                urllib.request.urlopen(cls.url + '/api/events', timeout=1).close()
                return
            except OSError:
                time.sleep(.1)
        raise RuntimeError('Server did not start')

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate()
        cls.process.wait(timeout=10)
        cls.temp.cleanup()

    def post(self, doc, ops):
        request = urllib.request.Request(self.url + '/api/events', data=json.dumps({'document': doc, 'operations': ops}).encode(), headers={'Content-Type': 'application/json'})
        return json.load(urllib.request.urlopen(request, timeout=5))

    def read(self, doc, after=0):
        return json.load(urllib.request.urlopen(self.url + f'/api/events?document={doc}&after={after}', timeout=5))['events']

    def test_isolation_cursor_and_idempotency(self):
        operation = dict(id='create', kind='document', type='fixed')
        self.post('one', [operation])
        self.post('two', [operation])
        rows = self.read('one')
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['op'], operation)
        self.assertEqual(self.read('one', rows[0]['seq']), [])
        self.post('one', [operation])
        self.assertEqual(self.read('one'), rows)
        self.assertEqual(len(self.read('two')), 1)

    def test_collision_rolls_back_entire_batch(self):
        old = dict(id='layer', kind='layer', layer='base', name='First', force=False)
        self.post('atomic', [old])
        new = dict(old, id='new', name='New')
        collision = dict(old, name='Different contents')
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.post('atomic', [new, collision])
        self.assertEqual(error.exception.code, 400)
        self.assertEqual([e['op'] for e in self.read('atomic')], [old])

    def test_reject_invalid_pressure_and_whole_batch(self):
        invalid = dict(id='bad', kind='stroke', stroke='s', layer='base', page=0, color=-16777216, width=5, points=[[1, 2, 2]])
        valid = dict(id='good', kind='document', type='fixed')
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.post('invalid', [valid, invalid])
        self.assertEqual(error.exception.code, 400)
        self.assertEqual(self.read('invalid'), [])

if __name__ == '__main__':
    unittest.main()

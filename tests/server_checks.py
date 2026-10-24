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
import base64
from io import BytesIO
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]

class ServerChecks(unittest.TestCase):
    def test_embedded_gif_is_validated_before_commit(self):
        pixels=[Image.new('RGB',(2,2),color) for color in ('white','black')]
        stream=BytesIO();pixels[0].save(stream,format='GIF',save_all=True,append_images=pixels[1:],duration=400,loop=0)
        gif='data:image/gif;base64,'+base64.b64encode(stream.getvalue()).decode()
        item=dict(id='gif-1',kind='object',object='gif-1',shape='gif',gif=gif,layer='base',page=0,
                  x=0,y=0,w=20,h=20,width=3,color=-16777216)
        self.post('gif-valid',[item])
        self.assertEqual(self.read('gif-valid')[0]['op']['gif'],gif)
        invalid=dict(item,id='gif-bad',object='gif-bad',gif='data:image/gif;base64,'+base64.b64encode(stream.getvalue()[:-1]).decode())
        with self.assertRaises(urllib.error.HTTPError) as error:self.post('gif-invalid',[invalid])
        self.assertEqual(error.exception.code,400)
        self.assertEqual(self.read('gif-invalid'),[])
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

    def test_deletion_index_is_atomic_and_retains_log(self):
        doc='delete-index'
        create=dict(id='create',kind='document',type='fixed')
        deletion=dict(id='delete',kind='document-delete',actor='human')
        self.post(doc,[create])
        with self.assertRaises(urllib.error.HTTPError):
            self.post(doc,[deletion,dict(create,type='pages')])
        def info():
            with urllib.request.urlopen(self.url+'/api/info') as response:
                return json.load(response)
        self.assertIn(doc,info()['documents'])
        self.assertNotIn(doc,info()['deletedDocuments'])
        self.assertEqual(len(self.read(doc)),1)
        self.post(doc,[deletion]);self.post(doc,[deletion])
        self.assertNotIn(doc,info()['documents'])
        self.assertIn(doc,info()['deletedDocuments'])
        self.assertEqual(len(self.read(doc)),2)
        self.post(doc,[dict(create,id='stale-create')])
        self.assertNotIn(doc,info()['documents'])

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
        invalid_material = dict(id='bad-material', kind='stroke', stroke='s', layer='base', page=0,
                                color=-16777216, width=5, brush='pencil', material=101,
                                points=[[1, 2, .5]])
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.post('invalid-material', [dict(valid, id='create'), invalid_material])
        self.assertEqual(error.exception.code, 400)
        self.assertEqual(self.read('invalid-material'), [])

    def test_editable_object_shapes_and_signed_line_bounds(self):
        shapes = ('line', 'triangle', 'diamond', 'pentagon', 'hexagon', 'star')
        operations = []
        for i, shape in enumerate(shapes):
            operations.append(dict(id='shape-' + shape, kind='object', object='object-' + shape,
                                   layer='base', shape=shape, x=10, y=20,
                                   w=-30 if shape == 'line' else 30,
                                   h=15 if shape == 'line' else 25,
                                   width=3, page=0, color=-16777216, fill=-1))
        self.post('shapes', operations)
        actual = [event['op'] for event in self.read('shapes')]
        self.assertEqual(actual, operations)

    def test_polygon_bounds_must_be_positive(self):
        invalid = dict(id='bad-polygon', kind='object', object='bad', layer='base', shape='triangle',
                       x=0, y=0, w=-10, h=20, width=2, page=0, color=0)
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.post('bad-polygon', [invalid])
        self.assertEqual(error.exception.code, 400)
        self.assertEqual(self.read('bad-polygon'), [])

if __name__ == '__main__':
    unittest.main()

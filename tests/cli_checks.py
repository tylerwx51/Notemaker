#!/usr/bin/env python3
"""Exercise actual CLI processes, replicas, server and image exporters."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import server_checks

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'apps/cli/notemaker'
EXAMPLE = json.loads((ROOT / 'apps/cli/examples/diagram.json').read_text())

class CliChecks(server_checks.ServerChecks):
    def test_environment_actor_and_explicit_override(self):
        env = dict(os.environ, NOTEMAKER_ACTOR='chat-agent')
        args = [str(CLI), '--state', str(self.directory / 'env'), '--document', self.document]
        result = subprocess.run(args + ['layer', 'put', '-'], env=env,
            input=json.dumps({'layer':'env-layer','name':'Environment actor','force':False}),
            text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        log = self.cli('log', replica='env')
        self.assertEqual(log['pending'][0]['actor'], 'chat-agent')
        result = subprocess.run(args + ['--actor', 'explicit', 'layer', 'put', '-'], env=env,
            input=json.dumps({'layer':'explicit-layer','name':'Explicit actor','force':False}),
            text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.cli('log', replica='env')['pending'][-1]['actor'], 'explicit')

    def test_svg_placement_history_export_and_sync(self):
        source = ROOT / 'apps/cli/examples/placed.svg'
        self.cli('document', 'create', 'pages')
        self.cli('layer', 'put', '-', value={'layer':'art','name':'Art','force':False}, actor='human')
        placement = self.directory / 'placement.json'
        placement.write_text(json.dumps({'object':'generated','x':100,'y':200,'w':400,'h':200,'layer':'art','page':1}))
        result = self.cli('svg', 'add', str(source), str(placement))
        self.assertIn('operation', result)
        m = self.cli('inspect');o=m['objects'][0]
        self.assertEqual((o['shape'],o['svg'],o['x'],o['y'],o['w'],o['h'],o['page'],o['layer']),('svg',source.read_text(),100,200,400,200,1,'art'))
        self.cli('undo', actor='human')  # independent history
        self.assertIsNot(self.cli('inspect')['objectActive'].get('generated'), False)
        self.cli('redo', actor='human')
        self.cli('undo');self.assertFalse(self.cli('inspect')['objectActive']['generated'])
        self.cli('redo')
        self.cli('object','update','generated','-',value={'x':120,'w':300},actor='editor')
        self.cli('undo',actor='editor');self.assertEqual(self.cli('inspect')['objects'][0]['x'],100)
        self.cli('export',str(self.directory/'svg.png'),'1')
        self.cli('export',str(self.directory/'svg.svg'),'1')
        self.assertIn('data:image/svg+xml;base64,',(self.directory/'svg.svg').read_text())
        self.cli('export',str(self.directory/'other-page.svg'),'0')
        self.assertNotIn('<image',(self.directory/'other-page.svg').read_text())
        self.cli('sync',self.url);self.cli('sync',self.url,replica='b')
        self.assertEqual(self.cli('inspect'),self.cli('inspect',replica='b'))
        self.cli('svg','add',str(source),str(placement),ok=False)  # no accidental replacement
        raw = subprocess.run([str(CLI),'--state',str(self.directory/'stdin'),'--document',self.document,'svg','add','-'],input=source.read_text(),text=True,capture_output=True,timeout=30)
        self.assertEqual(raw.returncode,0,raw.stderr)
        self.assertEqual(self.cli('inspect',replica='stdin')['objects'][0]['w'],200)

    def test_svg_rejections_are_atomic(self):
        self.diagram();before=self.cli('log')
        for source in [
            '<svg',
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 0 5"/>',
            '<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">&x;</svg>',
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><script>alert(1)</script></svg>',
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><rect width="20" height="20" fill="url(https://example.com/a)"/></svg>',
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><style>@import "https://example.com/a";</style></svg>',
            '<?xml-stylesheet href="file:///tmp/style.css"?><svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"/>',
            '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">'+' '*262144+'</svg>',
        ]:
            value={'object':'bad-svg','shape':'svg','svg':source}
            self.cli('object','put','-',value=value,ok=False)
            self.assertEqual(self.cli('log'),before)
            import urllib.error
            with self.assertRaises(urllib.error.HTTPError):
                self.post(self.document,[dict(value,id='bad',kind='object',layer='base',page=0,x=0,y=0,w=20,h=20,width=3,color=-16777216)])
        for placement in ({'page':1},{'layer':'missing'},{'w':0},{'x':1000001},{'rotation':90}):
            p=self.directory/'bad-placement.json';p.write_text(json.dumps(placement))
            self.cli('svg','add',str(ROOT/'apps/cli/examples/placed.svg'),str(p),ok=False)
            self.assertEqual(self.cli('log'),before)

    def test_svg_intrinsic_size_and_isolated_export(self):
        import base64
        import xml.etree.ElementTree as ET
        source='<?xml version="1.0"?><s:svg xmlns:s="http://www.w3.org/2000/svg" width="80px" height="40px"><!-- <?not-a-pi ?> --><s:rect width="80" height="40"/></s:svg>'
        self.cli('object','put','-',value={'shape':'svg','object':'pixel-sized','svg':source})
        o=self.cli('inspect')['objects'][0]
        self.assertEqual((o['w'],o['h'],o['svg']),(80,40,source))
        self.cli('export',str(self.directory/'embedded.svg'))
        root=ET.parse(self.directory/'embedded.svg').getroot()
        image=root.find('{http://www.w3.org/2000/svg}image')
        embedded=ET.fromstring(base64.b64decode(image.attrib['{http://www.w3.org/1999/xlink}href'].split(',')[1]))
        self.assertEqual(embedded.attrib['viewBox'],'0 0 80.0 40.0')
        self.assertEqual(embedded.attrib['width'],'100%')
        self.cli('sync',self.url)

    def setUp(self):
        self.local = tempfile.TemporaryDirectory()
        self.directory = Path(self.local.name)
        self.document = self.id().split('.')[-1]

    def tearDown(self):
        self.local.cleanup()

    def cli(self, *args, replica='a', actor='agent', value=None, ok=True):
        p = subprocess.run([str(CLI), '--state', str(self.directory / replica), '--document', self.document,
                            '--actor', actor, *args], input=json.dumps(value) if value is not None else None,
                           text=True, capture_output=True, timeout=30)
        if ok:
            self.assertEqual(p.returncode, 0, p.stderr)
            return json.loads(p.stdout)
        self.assertNotEqual(p.returncode, 0, p.stdout)
        return json.loads(p.stderr)

    def diagram(self, **kwargs):
        return self.cli('diagram', 'add', '-', value=EXAMPLE, **kwargs)

    def test_offline_diagram_edit_history_and_render(self):
        self.cli('document', 'create', 'pages')
        self.diagram()
        m = self.cli('inspect')
        self.assertEqual(len(m['objects']), 8)
        self.cli('object', 'update', 'app', '-', value={'x': 150, 'text': 'Changed label'}, actor='editor')
        self.cli('undo', actor='editor')
        self.assertEqual(self.cli('inspect')['objects'][0]['text'], 'NoteMaker app')
        self.cli('redo', actor='editor')
        self.assertEqual(self.cli('inspect')['objects'][0]['x'], 150)
        self.cli('undo')
        m = self.cli('inspect')
        self.assertFalse(m['layerActive']['architecture'])
        self.assertTrue(all(not v for v in m['objectActive'].values()))
        self.cli('redo')
        m = self.cli('inspect')
        self.assertTrue(m['layerActive']['architecture'])
        self.assertTrue(all(m['objectActive'].values()))
        self.cli('export', str(self.directory / 'drawing.png'))
        self.assertTrue((self.directory / 'drawing.png').read_bytes().startswith(b'\x89PNG\r\n\x1a\n'))
        self.cli('export', str(self.directory / 'drawing.svg'))
        self.assertIn('NoteMaker app', (self.directory / 'drawing.svg').read_text())
        self.cli('export', str(self.directory / 'page-two.svg'), '1')
        self.assertNotIn('NoteMaker app', (self.directory / 'page-two.svg').read_text())

    def test_concurrent_local_writers(self):
        def create(i):
            return self.cli('object', 'put', '-', actor=f'agent-{i}', value={'object':f'node-{i}', 'shape':'rect', 'x':i*20})
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            list(pool.map(create, range(24)))
        self.assertEqual(len(self.cli('inspect')['objects']), 24)
        self.assertEqual(self.cli('status')['pending'], 24)
        self.cli('undo', actor='agent-3')
        m = self.cli('inspect')
        self.assertFalse(m['objectActive']['node-3'])
        self.assertTrue(m['objectActive']['node-4'])

    def test_reconnect_conflict_convergence_and_retry(self):
        self.diagram()
        self.cli('sync', self.url)
        self.cli('sync', self.url, replica='b')
        self.cli('object', 'update', 'app', '-', value={'x':222}, replica='a')
        self.cli('object', 'update', 'app', '-', value={'x':444}, replica='b')
        self.cli('object', 'hide', 'app', replica='b')
        self.cli('sync', self.url, replica='a')
        self.cli('sync', self.url, replica='b')
        self.cli('sync', self.url, replica='a')
        a,b = self.cli('inspect'),self.cli('inspect', replica='b')
        self.assertEqual(a,b)
        self.assertEqual(a['objects'][0]['x'],444)
        self.assertFalse(a['objectActive']['app'])
        count=len(self.read(self.document))
        self.cli('sync', self.url)
        self.assertEqual(len(self.read(self.document)),count)
        self.assertEqual(self.cli('status')['pending'],0)
        self.cli('sync','http://127.0.0.1:1',ok=False)
        self.assertEqual(self.cli('inspect'),a)

    def test_reject_partial_invalid_action(self):
        self.diagram()
        before=self.cli('log')
        self.cli('apply','-',value=[{'kind':'layer','layer':'bad','name':'Bad','force':False}, {'kind':'object','object':'x'}],ok=False)
        self.assertEqual(self.cli('log'),before)
        self.cli('object','update','app','-',value={'w':-1},ok=False)
        self.cli('apply','-',value=[{'kind':'batch','actions':[{'kind':'layer','layer':'nested','name':'Nested','force':False}]}],ok=False)
        self.assertEqual(self.cli('log'),before)
        self.cli('object','update','app','-',value={'text':'<&label>'})
        self.cli('export',str(self.directory/'escaped.svg'))
        self.assertIn('&lt;&amp;label&gt;',(self.directory/'escaped.svg').read_text())

    def test_server_rejects_invalid_nested_batch_atomically(self):
        import urllib.error
        with self.assertRaises(urllib.error.HTTPError):
            self.post(self.document,[{'id':'transaction','kind':'batch','actions':[{'kind':'layer','layer':'ok','name':'OK','force':False},{'kind':'object','object':'invalid'}]}])
        self.assertEqual(self.read(self.document),[])

    def test_edit_during_sync_and_lost_ack(self):
        import threading
        import urllib.request
        from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
        entered, release = threading.Event(), threading.Event()
        upstream = self.url
        lose_ack = [False]
        class Proxy(BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path.startswith('/api/events'):
                    entered.set()
                    assert release.wait(10)
                data=urllib.request.urlopen(upstream+self.path,timeout=5).read()
                self.send_response(200);self.end_headers();self.wfile.write(data)
            def do_POST(self):
                body=self.rfile.read(int(self.headers['Content-Length']))
                data=urllib.request.urlopen(urllib.request.Request(upstream+self.path,data=body,headers={'Content-Type':'application/json'}),timeout=5).read()
                if lose_ack[0]:
                    self.close_connection=True
                    return
                self.send_response(200);self.end_headers();self.wfile.write(data)
            def log_message(self,*args):pass
        proxy=ThreadingHTTPServer(('127.0.0.1',0),Proxy)
        thread=threading.Thread(target=proxy.serve_forever,daemon=True);thread.start()
        url=f'http://127.0.0.1:{proxy.server_port}'
        try:
            self.diagram()
            with concurrent.futures.ThreadPoolExecutor() as pool:
                sync=pool.submit(self.cli,'sync',url)
                self.assertTrue(entered.wait(5))
                self.cli('object','update','app','-',value={'x':321})
                release.set();sync.result()
            self.assertEqual(self.cli('status')['pending'],1)
            self.assertEqual(self.cli('inspect')['objects'][0]['x'],321)
            lose_ack[0]=True
            self.cli('sync',url,ok=False)
            count=len(self.read(self.document))
            self.assertEqual(self.cli('status')['pending'],1)
            self.cli('sync',self.url)
            self.assertEqual(len(self.read(self.document)),count)
            self.assertEqual(self.cli('status')['pending'],0)
        finally:
            release.set();proxy.shutdown();proxy.server_close();thread.join()

    def test_server_identity_and_connector_validation(self):
        self.diagram()
        self.cli('object','update','app-state','-',value={'to':'missing'},ok=False)
        self.cli('object','update','app','-',value={'shape':'arrow'},ok=False)
        self.cli('sync',self.url)
        path=next((self.directory/'a').glob('document-*.json'))
        state=self.cli("log");state['serverId']='different-database';path.write_text(json.dumps(state));Path(str(path)+".journal").unlink()
        self.cli('object','update','app','-',value={'x':222})
        count=len(self.read(self.document))
        error=self.cli('sync',self.url,ok=False)
        self.assertIn('database differs',error['error'])
        self.assertEqual(len(self.read(self.document)),count)
        self.assertEqual(self.cli('status')['pending'],1)

    def test_pagination_and_legacy_replica_migration(self):
        for start in range(0,1100,100):
            self.post(self.document,[dict(id=f'layer-{i}',kind='layer',layer=f'l-{i}',name='Layer',force=False) for i in range(start,start+100)])
        self.cli('sync',self.url)
        self.assertEqual(len(self.cli('inspect')['layers']),1101)
        self.assertEqual(len(self.cli('log')['events']),1100)
        # Existing Android files lack actor/revision/document fields; preserve their outbox.
        import uuid,hashlib
        directory=self.directory/'legacy';directory.mkdir()
        name=str(uuid.UUID(bytes=hashlib.md5(self.document.encode()).digest(),version=3))
        (directory/f'document-{name}.json').write_text(json.dumps({'events':[],'pending':[{'id':'old','kind':'layer','layer':'old','name':'Old','force':False}],'cursor':0,'hidden':['old'],'undo':[],'redo':[]}))
        self.cli('layer','put','-',value={'layer':'new','name':'New','force':False},replica='legacy')
        self.assertEqual(self.cli('log',replica='legacy')['hidden'],['old'])
        self.assertEqual(len(self.cli('inspect',replica='legacy')['layers']),3)

if __name__ == '__main__':
    unittest.main()

#!/usr/bin/env python3
"""Exercise new editable shapes through the real JVM CLI and local proxies."""
import json
import base64
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from server_checks import ServerChecks
from topology_support import Proxy

ROOT=Path(__file__).resolve().parents[1]
CLI=ROOT/'apps/cli/notemaker'
SHAPES=('line','triangle','diamond','pentagon','hexagon','star')

class DrawingChecks(ServerChecks):
    def setUp(self):
        self.local=tempfile.TemporaryDirectory();self.root=Path(self.local.name)
        self.document='shapes-'+self.id().split('.')[-1];self.proxies=[]

    def tearDown(self):
        for proxy in self.proxies:proxy.stop()
        self.local.cleanup()

    def proxy(self, name):
        proxy=Proxy(self.root/name,self.document);self.proxies.append(proxy);return proxy

    def call(self, proxy, *args, actor='shape-editor', value=None, ok=True, direct=False):
        options=['--state',str(proxy.directory)] if direct else ['--proxy-file',str(proxy.descriptor_path)]
        result=subprocess.run([str(CLI),*options,'--document',self.document,'--actor',actor,*args],
            input=json.dumps(value) if value is not None else None,text=True,capture_output=True,timeout=40)
        self.assertEqual(result.returncode==0,ok,result.stderr or result.stdout)
        return json.loads(result.stdout if ok else result.stderr)

    def test_shape_catalog_crud_history_render_and_proxy_sync(self):
        a=self.proxy('shape-a');b=self.proxy('shape-b')
        expected=set(SHAPES)
        catalog=self.call(a,'shape','list');self.assertTrue(expected.issubset({entry['id'] for entry in catalog['shapes']}))
        self.assertEqual(next(item['name'] for item in catalog['shapes'] if item['id']=='triangle'),'Triangle')
        self.assertEqual(self.call(a,'shape','list',direct=True),self.call(a,'shape','list'))
        before=self.call(a,'log')
        self.call(a,'shape','list')
        self.assertEqual(self.call(a,'log'),before)
        self.call(a,'object','put','-',value={'object':'signed-line','shape':'line','x':600,'y':300,'w':-80,'h':0,'width':3})
        self.assertEqual((next(o for o in self.call(a,'inspect')['objects'] if o['object']=='signed-line')['w'],
                          next(o for o in self.call(a,'inspect')['objects'] if o['object']=='signed-line')['h']),(-80,0))
        unchanged=self.call(a,'log')
        self.call(a,'apply','-',value=[{'kind':'object','object':'atomic-valid','shape':'rect','layer':'base','page':0,
            'x':1,'y':2,'w':3,'h':4,'color':-16777216,'width':3},
            {'kind':'object','object':'bad-triangle','shape':'triangle','layer':'base','page':0,
            'x':1,'y':2,'w':-3,'h':4,'color':-16777216,'width':3}],ok=False)
        self.assertEqual(self.call(a,'log'),unchanged)
        for index,shape in enumerate(SHAPES):
            self.call(a,'object','put','-',value={'object':shape,'shape':shape,'x':80+index*130,
                'y':150,'w':90,'h':100,'color':-16777216,'width':4,'fill':-2147418368,
                'text':shape.title(),'fontSize':18})
        model=self.call(a,'inspect')
        self.assertEqual({o['shape'] for o in model['objects']},expected)
        self.call(a,'object','update','triangle','-',value={'x':444,'fill':-1},actor='geometry-editor')
        self.call(a,'undo',actor='geometry-editor')
        self.assertEqual(next(o for o in self.call(a,'inspect')['objects'] if o['object']=='triangle')['x'],210)
        self.call(a,'redo',actor='geometry-editor')
        self.assertEqual(next(o for o in self.call(a,'inspect')['objects'] if o['object']=='triangle')['x'],444)
        svg=self.root/'shapes.svg';png=self.root/'shapes.png'
        self.call(a,'export',str(svg));self.call(a,'export',str(png))
        source=svg.read_text()
        encoded=re.findall(r'xlink:href="data:image/svg\+xml;base64,([^"]+)',source)
        self.assertEqual(len(encoded),5)
        polygons=[]
        for image in encoded:
            nested=base64.b64decode(image).decode()
            points=re.search(r'<(?:[A-Za-z0-9_]+:)?polygon[^>]*points="([^"]+)',nested).group(1)
            polygons.append(points)
        self.assertEqual(len(set(polygons)),5)
        self.assertGreater(png.stat().st_size,100)
        self.call(a,'undo')
        self.assertFalse(self.call(a,'inspect')['objectActive']['star'])
        self.call(a,'redo')
        self.assertTrue(self.call(a,'inspect')['objectActive']['star'])
        self.call(a,'sync',self.url)
        self.call(b,'sync',self.url)
        self.assertEqual(self.call(b,'inspect'),self.call(a,'inspect'))
        self.assertEqual(self.call(b,'status')['pending'],0)
        self.call(a,'object','put','-',value={'object':'maximum-polygon','shape':'triangle','x':0,'y':0,
            'w':1000000,'h':1000000,'width':100})
        large=self.root/'maximum-polygon.svg';self.call(a,'export',str(large))
        embedded=re.findall(r'xlink:href="data:image/svg\+xml;base64,([^"]+)',large.read_text())[-1]
        nested=base64.b64decode(embedded).decode()
        viewbox=list(map(float,re.search(r'viewBox="([^"]+)"',nested).group(1).split()))
        self.assertLessEqual(max(viewbox[2:]),1000)

if __name__=='__main__':unittest.main()

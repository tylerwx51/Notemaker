#!/usr/bin/env python3
"""Real CLI coverage for shared erasing, fills, layers, templates and raster assets."""
import base64
import json
from pathlib import Path
import tempfile
import struct
import zlib
import unittest
from topology_support import Proxy

class DrawingToolsChecks(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.path=Path(self.temp.name)
        self.proxy=Proxy(self.path/'replica','tools')
    def tearDown(self):
        self.proxy.stop();self.temp.cleanup()
    def call(self,*args,value=None,ok=True):return self.proxy.cli(*args,value=value,ok=ok)
    def stroke(self,id='s',points=None):
        self.call('stroke','put','-',value=dict(stroke=id,layer='base',page=0,width=2,color=-16777216,points=points or [[0,50,1],[100,50,1]]))
    def test_partial_eraser_masks_and_undo_restores_as_one_action(self):
        self.stroke();before=self.call('log')
        self.call('erase','-',value=dict(mode='partial',layer='base',points=[[50,0],[50,100]],radius=9))
        state=self.call('inspect');self.assertNotEqual(state['strokeActive'].get('s'),False)
        self.assertEqual(len(state['strokes']),1);self.assertEqual(len(state['strokes'][0]['erasures']),1)
        query=dict(layer='base',region=[[49,49],[51,49],[51,51],[49,51]])
        self.assertEqual(self.call('selection','query','-',value=query)['items'],[])
        after=self.call('log');self.assertEqual(len(after['pending']),len(before['pending'])+1)
        self.call('undo');state=self.call('inspect');self.assertNotIn('erasures',state['strokes'][0])
        self.assertEqual(self.call('selection','query','-',value=query)['items'][0]['id'],'s')
        self.call('redo');self.assertEqual(self.call('selection','query','-',value=query)['items'],[])
    def test_stroke_and_page_erase_cancel_on_invalid_input(self):
        self.stroke();before=self.call('log')
        self.call('erase','-',value=dict(mode='partial',points=[[50,50]],radius=0),ok=False)
        self.assertEqual(before,self.call('log'))
        self.call('erase','-',value=dict(mode='stroke',points=[[50,50]],radius=4))
        self.assertFalse(self.call('inspect')['strokeActive']['s']);self.call('undo')
        self.call('object','put','-',value=dict(object='box',shape='rect',x=0,y=0,w=10,h=10))
        self.call('erase','-',value=dict(mode='page',page=0))
        state=self.call('inspect');self.assertFalse(state['strokeActive']['s']);self.assertFalse(state['objectActive']['box'])
        self.call('undo');self.assertTrue(self.call('inspect')['objectActive']['box'])
    def test_native_and_closed_stroke_fill(self):
        self.call('object','put','-',value=dict(object='box',shape='rect',x=10,y=10,w=50,h=50))
        self.call('fill','-',value=dict(object='box',color=-65536));self.assertEqual(self.call('inspect')['objects'][0]['fill'],-65536)
        self.stroke(points=[[0,0,1],[100,0,1],[100,100,1],[0,100,1],[0,0,1]])
        self.call('fill','-',value=dict(stroke='s',color=-16711936));state=self.call('inspect')
        self.assertEqual(len(state['objects']),2);self.assertIn('#00ff00',state['objects'][1]['svg'])
        self.call('undo');self.assertFalse(self.call('inspect')['objectActive'][state['objects'][1]['object']])
        self.stroke('open',[[0,0,1],[100,0,1],[100,100,1]])
        before=self.call('log');self.call('fill','-',value=dict(stroke='open',color=-1),ok=False);self.assertEqual(before,self.call('log'))
    def test_layer_duplicate_merge_and_order_are_atomic(self):
        self.call('layer','put','-',value=dict(layer='source',name='Source',force=False))
        self.call('object','put','-',value=dict(object='box',shape='rect',layer='source',x=10,y=10,w=50,h=50))
        self.call('layer','duplicate','source');state=self.call('inspect');copy=next(l for l in state['layers'] if l['name']=='Source copy')
        self.assertEqual(len(state['objects']),2);self.assertEqual(state['objects'][1]['layer'],copy['layer'])
        self.call('layer','reorder',copy['layer'],'0');self.assertEqual(next(l for l in self.call('inspect')['layers'] if l['layer']==copy['layer'])['order'],0)
        self.call('layer','merge',copy['layer'],'base');state=self.call('inspect');self.assertFalse(state['layerActive'][copy['layer']]);self.assertEqual(state['objects'][1]['layer'],'base')
        self.call('undo');self.assertTrue(self.call('inspect')['layerActive'][copy['layer']])
    def test_fill_region_closed_by_multiple_strokes_and_reject_open_space(self):
        for i,points in enumerate(([[10,10,1],[110,10,1]],[[110,10,1],[110,110,1]],[[110,110,1],[10,110,1]],[[10,110,1],[10,10,1]])):
            self.stroke(str(i),points)
        self.call('fill','-',value=dict(x=50,y=50,layer='base',page=0,color=-65536))
        state=self.call('inspect');self.assertEqual(len(state['objects']),1);self.assertIn('#ff0000',state['objects'][0]['svg'])
        before=self.call('log');self.call('fill','-',value=dict(x=150,y=150,layer='base',page=0,color=-65536),ok=False);self.assertEqual(before,self.call('log'))
        self.call('undo');self.assertFalse(self.call('inspect')['objectActive'][state['objects'][0]['object']])
    def test_excessively_complex_fill_is_rejected_without_an_action(self):
        self.stroke(points=[[0,0,1] if i%2==0 else [1000,1000,1] for i in range(100)])
        before=self.call('log');error=self.call('fill','-',value=dict(x=50,y=60,page=0,layer='base',color=-1),ok=False)
        self.assertIn('too complex',str(error));self.assertEqual(before,self.call('log'))
    def test_templates_and_png_export(self):
        for i,kind in enumerate(('blank','ruled','grid','dots','planner')):
            self.call('template','add','-',value=dict(template=kind,object='template-'+kind,x=i*200,y=0,w=180,h=250,spacing=40))
        def chunk(kind,data): return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
        png=self.path/'tiny.png';png.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b'\x00\xff\x00\x00\xff'))+chunk(b'IEND',b''))
        placement=self.path/'placement.json';placement.write_text(json.dumps(dict(object='picture',x=10,y=300,w=40,h=40)))
        self.call('image','add',str(png),str(placement));state=self.call('inspect');self.assertEqual(state['objects'][-1]['shape'],'image')
        self.call('export',str(self.path/'tools.svg'));self.call('export',str(self.path/'tools.png'))
        self.assertIn('data:image/png;base64,',(self.path/'tools.svg').read_text());self.assertGreater((self.path/'tools.png').stat().st_size,100)
        self.call('object','put','-',value=dict(shape='image',image='data:image/png;base64,aGVsbG8='),ok=False)
    def test_new_style_validation_and_preservation(self):
        self.call('object','put','-',value=dict(object='text',shape='text',text='Hello',bold=True,italic=True,underline=True,fontFamily='serif',alignment='center'))
        for brush in ('pen','pencil','marker','brush','oily','fountain','charcoal','neoBrush','latinCalligraphy','asiaCalligraphy','square'):
            self.call('stroke','put','-',value=dict(stroke=brush,layer='base',page=0,width=5,color=-16777216,brush=brush,points=[[1,1,.2],[10,10,.9]]))
        self.assertEqual(len(self.call('inspect')['strokes']),11)
        self.call('object','update','text','-',value=dict(alignment='invalid'),ok=False)
        self.call('export',str(self.path/'styled.png'));self.assertGreater((self.path/'styled.png').stat().st_size,100)
    def test_material_parameters_validate_and_change_rendered_strokes(self):
        points=[[20,20,.25],[35,28,.7],[50,20,.95],[65,34,.45],[80,25,.8]]
        low=dict(stroke='material-sample',layer='base',page=0,width=8,color=-16777216,brush='pencil',material=0,points=points)
        high={**low,'material':100}
        self.call('stroke','put','-',value=low)
        low_svg=self.path/'material-low.svg';self.call('export',str(low_svg))
        self.call('stroke','put','-',value=high)
        high_svg=self.path/'material-high.svg';self.call('export',str(high_svg))
        self.assertNotEqual(low_svg.read_text(),high_svg.read_text())
        saved=next(s for s in self.call('inspect')['strokes'] if s['stroke']=='material-sample')
        self.assertEqual(saved['material'],100)
        before=self.call('log');self.call('stroke','put','-',value={**high,'material':101},ok=False)
        self.assertEqual(before,self.call('log'))
    def test_material_paths_do_not_accumulate_with_dense_input_samples(self):
        sparse=[[20,50,.8],[180,50,.8]]
        dense=[[20+i,50,.8] for i in range(161)]
        for brush in ('pen','pencil','marker','brush','oily','fountain','charcoal','neoBrush','latinCalligraphy'):
            stroke=dict(stroke='density-'+brush,layer='base',page=0,width=10,color=-16777216,
                        brush=brush,material=65,points=sparse)
            self.call('stroke','put','-',value=stroke)
            sparse_png=self.path/(brush+'-sparse.png');self.call('export',str(sparse_png))
            self.call('stroke','put','-',value={**stroke,'points':dense})
            dense_png=self.path/(brush+'-dense.png');self.call('export',str(dense_png))
            self.assertEqual(sparse_png.read_bytes(),dense_png.read_bytes(),brush+' changed with sample density')
    def test_clipboard_cut_paste_preserves_atomic_diagram_and_references(self):
        self.call('document','create','pages')
        self.call('diagram','add','-',value=dict(layer='diagram',name='Diagram',objects=[
            dict(object='left',shape='rect',x=10,y=10,w=40,h=30),
            dict(object='right',shape='rect',x=100,y=10,w=40,h=30),
            dict(object='link',shape='arrow',**{'from':'left','to':'right'})]))
        self.call('selection','select','-',value=dict(session='clipboard',layer='diagram',page=0,entities=[dict(kind='object',id=id) for id in ('left','right','link')]))
        copied=self.call('selection','copy','clipboard');self.assertEqual(len(copied['entities']),3)
        before=self.call('log');self.call('selection','paste','-',value=dict(copied,page=1,layer='base',dx=20,dy=30))
        after=self.call('log');self.assertEqual(len(after['pending']),len(before['pending'])+1)
        pasted=[o for o in self.call('inspect')['objects'] if o['page']==1];self.assertEqual(len(pasted),3)
        arrow=next(o for o in pasted if o['shape']=='arrow');new_ids={o['object'] for o in pasted}
        self.assertIn(arrow['from'],new_ids);self.assertIn(arrow['to'],new_ids);self.assertNotIn(arrow['from'],('left','right'))
        cut=self.call('selection','cut','clipboard');self.assertEqual(len(cut['entities']),3)
        self.assertTrue(all(self.call('inspect')['objectActive'][id] is False for id in ('left','right','link')))
        self.call('undo');self.assertTrue(all(self.call('inspect')['objectActive'][id] is True for id in ('left','right','link')))

if __name__=='__main__':unittest.main()

#!/usr/bin/env python3
"""Coverage and layer scope through real offline CLI/proxy transactions."""
import base64
import struct
import tempfile
import unittest
import zlib
from pathlib import Path
from topology_support import Proxy
from drawing_browser import read_rgba_png


class CoverageChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.proxy = Proxy(self.root / 'replica', 'coverage')

    def tearDown(self):
        self.proxy.stop()
        self.temp.cleanup()

    def call(self, *args, value=None, ok=True):
        return self.proxy.cli(*args, value=value, ok=ok)

    def put(self, **item):
        self.call('object', 'put', '-', value=item)

    def query(self, x, y, layer='base', **options):
        result = self.call('selection', 'query', '-', value=dict(
            layer=layer, page=0, region=[[x-.1,y-.1],[x+.1,y-.1],[x+.1,y+.1],[x-.1,y+.1]], **options))
        return [item['id'] for item in result['items']]

    def test_hollow_shape_pressure_ink_and_topmost_layer_candidates(self):
        self.call('layer','put','-',value=dict(layer='other',name='Other',force=False,order=-1))
        self.put(object='under',shape='rect',layer='other',x=0,y=0,w=100,h=100,fill=-1)
        self.put(object='ring',shape='ellipse',x=0,y=0,w=100,h=100,width=4)
        self.assertEqual(self.query(50,50), [])
        self.assertEqual(self.query(0,50), ['ring'])
        self.assertEqual(self.query(50,50,'other'), ['under'])
        self.call('stroke','put','-',value=dict(stroke='ink',layer='base',page=0,width=20,color=-16777216,points=[[50,50,0]]))
        self.assertEqual(self.query(50,52), ['ink'])
        self.assertEqual(self.query(50,56), [])
        self.put(object='top',shape='rect',x=45,y=45,w=10,h=10,fill=-1)
        self.assertEqual(self.query(50,50,topmost=True), ['top'])

    def test_image_alpha_white_pixels_holes_rotation_and_svg(self):
        def chunk(kind,data):
            return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
        pixels=b''.join(b'\0'+b''.join(bytes([255,255,255,0 if x in (1,2) and y in (1,2) else 255]) for x in range(4)) for y in range(4))
        png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',4,4,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(pixels))+chunk(b'IEND',b'')
        self.put(object='image',shape='image',x=10,y=10,w=40,h=40,image='data:image/png;base64,'+base64.b64encode(png).decode())
        self.assertEqual(self.query(30,30), [])
        self.assertEqual(self.query(12,12), ['image'])
        self.call('object','update','image','-',value=dict(rotation=45))
        self.assertEqual(self.query(30,30), [])
        self.assertEqual(self.query(30,5), ['image'])
        self.put(object='svg',shape='svg',x=100,y=100,w=100,h=100,svg='<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><circle cx="50" cy="50" r="40" fill="none" stroke="black" stroke-width="4"/></svg>')
        self.assertEqual(self.query(150,150), [])
        self.assertEqual(self.query(110,150), ['svg'])

    def test_partial_erase_keeps_identity_other_layer_and_atomic_undo(self):
        self.call('layer','put','-',value=dict(layer='other',name='Other',force=False,order=-1))
        for layer,id in [('base','box'),('other','under')]:
            self.put(object=id,layer=layer,shape='rect',x=0,y=0,w=100,h=100,fill=-16777216 if layer=='base' else -65536)
        self.call('stroke','put','-',value=dict(stroke='ink',layer='base',page=0,width=4,color=-16777216,points=[[0,50,1],[100,50,1]]))
        before=self.call('log')
        self.call('erase','-',value=dict(mode='partial',layer='base',page=0,radius=10,points=[[50,0],[50,100]]))
        state=self.call('inspect')
        self.assertEqual(len(state['strokes']),1)
        self.assertEqual(state['strokes'][0]['stroke'],'ink')
        self.assertEqual(self.query(50,50),[])
        self.assertEqual(self.query(50,50,'other'),['under'])
        self.assertEqual(set(self.query(20,50)),{'box','ink'})
        self.assertEqual(len(self.call('log')['pending']),len(before['pending'])+1)
        self.call('export',str(self.root/'masked.svg'))
        self.assertIn('clipPath',(self.root/'masked.svg').read_text())
        self.call('export',str(self.root/'masked.png'))
        _,_,pixel=read_rgba_png(self.root/'masked.png')
        self.assertEqual(pixel(50,50)[:3],(255,0,0))
        self.assertEqual(pixel(20,50)[:3],(0,0,0))
        self.call('undo')
        self.assertEqual(set(self.query(50,50)),{'box','ink'})
        self.call('redo')
        self.assertEqual(self.query(50,50),[])
        self.proxy.stop();self.proxy.start()
        self.assertEqual(self.query(50,50),[])

    def test_masks_follow_object_resize_and_ink_rotation(self):
        self.put(object='box',shape='rect',x=0,y=0,w=100,h=100,fill=-1)
        self.call('erase','-',value=dict(mode='partial',radius=10,points=[[25,0],[25,100]]))
        self.call('object','update','box','-',value=dict(x=200,w=200))
        self.assertEqual(self.query(250,50),[])
        self.assertEqual(self.query(350,50),['box'])
        self.call('stroke','put','-',value=dict(stroke='ink',layer='base',page=0,width=4,color=-16777216,points=[[0,50,1],[100,50,1]]))
        self.call('erase','-',value=dict(mode='partial',radius=10,points=[[50,50]]))
        refs=[dict(kind='stroke',id='ink')]
        self.call('selection','select','-',value=dict(session='rotate',layer='base',page=0,entities=refs))
        self.call('transform','commit','-',value=dict(session='rotate',entities=refs,transform=dict(rotation=90,pivotX=0,pivotY=0)))
        self.assertEqual(self.query(-50,50),[])
        self.assertEqual(self.query(-50,20),['ink'])

    def test_wrong_layer_batch_rejected_before_leases_and_session_cannot_change_scope(self):
        self.call('layer','put','-',value=dict(layer='other',name='Other',force=False))
        self.put(object='a',shape='rect',layer='base')
        self.put(object='b',shape='rect',layer='other')
        before=self.call('log')
        self.call('object','update','b','-',value=dict(x=20),ok=False)
        self.call('object','hide','b',ok=False)
        self.assertEqual(before,self.call('log'))
        self.call('object','update','b','-',value=dict(scope=dict(layer='other',page=0),x=20))
        self.call('selection','select','-',value=dict(session='mixed',layer='base',entities=[dict(kind='object',id='a'),dict(kind='object',id='b')]),ok=False)
        self.assertEqual(self.call('snapshot')['leases'],{})
        self.call('selection','select','-',value=dict(session='one',layer='base',entities=[dict(kind='object',id='a')]))
        self.call('selection','select','-',value=dict(session='one',layer='other',entities=[dict(kind='object',id='b')]),ok=False)

    def test_erase_transparent_interior_is_noop_and_invalid_masks_are_atomic(self):
        self.put(object='ring',shape='ellipse',x=0,y=0,w=100,h=100,width=3)
        before=self.call('log')
        self.call('erase','-',value=dict(mode='item',radius=3,points=[[50,50]]),ok=False)
        self.call('object','update','ring','-',value=dict(erasures=[[[0,0],[1,1]]]),ok=False)
        self.assertEqual(before,self.call('log'))
        self.call('erase','-',value=dict(mode='item',radius=3,points=[[0,50]]))
        self.assertFalse(self.call('inspect')['objectActive']['ring'])

    def test_empty_coverage_and_flood_fill_respect_erased_boundaries(self):
        self.put(object='box',shape='rect',x=0,y=0,w=100,h=100,width=4)
        self.call('erase','-',value=dict(mode='partial',radius=10,points=[[50,0]]))
        self.call('fill','-',value=dict(x=50,y=50,layer='base',page=0,color=-1),ok=False)
        self.call('erase','-',value=dict(mode='partial',radius=100,points=[[50,50]]))
        self.assertEqual(self.call('selection','query','-',value=dict(layer='base'))['items'],[])
        self.call('undo')
        self.assertEqual(self.query(0,50),['box'])

    def test_text_spaces_and_transparent_paint_are_not_coverage(self):
        self.put(object='spaces',shape='text',text='   ',x=0,y=0,w=200,h=100,fontSize=40)
        self.put(object='clear',shape='rect',x=0,y=0,w=100,h=100,color=0,fill=0)
        self.assertEqual(self.query(0,50),[])
        self.assertEqual(self.query(20,20),[])
        self.put(object='text',shape='text',text='O',x=150,y=0,w=100,h=100,fontSize=60)
        hits=self.call('selection','query','-',value=dict(layer='base',region=[[145,0],[215,0],[215,70],[145,70]]))['items']
        self.assertEqual([item['id'] for item in hits],['text'])

    def test_offline_masks_retain_both_conflicting_versions_on_reconnect(self):
        self.put(object='shared',shape='rect',x=0,y=0,w=100,h=100,fill=-1)
        peer=Proxy(self.root/'peer','coverage')
        try:
            peer.cli('sync',str(self.proxy.descriptor_path))
            peer.cli('configure','')
            self.call('erase','-',value=dict(mode='partial',radius=8,points=[[25,50]]))
            peer.cli('erase','-',value=dict(mode='partial',radius=8,points=[[75,50]]))
            peer.cli('sync',str(self.proxy.descriptor_path))
            conflicts=self.call('conflicts')['conflicts']
            self.assertEqual(len(conflicts),1)
            versions=[conflicts[0][side]['erasures'] for side in ('local','remote')]
            self.assertTrue(all(len(mask)==1 for mask in versions))
            self.assertNotEqual(versions[0],versions[1])
        finally:
            peer.stop()

    def test_repeated_collinear_and_backtracking_erase_points(self):
        self.put(object='box',shape='rect',x=0,y=0,w=100,h=100,fill=-1)
        self.call('erase','-',value=dict(mode='partial',radius=3,points=[[10,50],[10,50],[30,50],[70,50],[30,50],[30,80]]))
        for x,y in [(10,50),(50,50),(70,50),(30,70)]:
            self.assertEqual(self.query(x,y),[])
        self.assertEqual(self.query(50,70),['box'])
        self.call('undo')
        self.assertEqual(self.query(50,50),['box'])

    def test_appended_ink_does_not_reuse_old_action_metadata(self):
        self.call('append','-',value=dict(kind='stroke',stroke='raw',layer='base',page=0,width=4,color=-16777216,points=[[0,50,1],[100,50,1]]))
        original=self.call('inspect')['strokes'][0]
        self.assertIn('id',original)
        self.call('erase','-',value=dict(mode='partial',radius=10,points=[[50,50]]))
        masked=self.call('inspect')['strokes'][0]
        self.assertNotIn('id',masked)
        self.assertNotIn('actor',masked)
        self.assertEqual(masked['stroke'],'raw')
        self.call('undo')
        self.assertEqual(self.call('inspect')['strokes'][0],original)
        self.call('redo')
        self.assertEqual(self.query(50,50),[])


if __name__=='__main__':
    unittest.main()

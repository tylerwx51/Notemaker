#!/usr/bin/env python3
"""Verify editable shape pixels in the live server viewer via CLI and proxy."""
import json
import base64
import re
import struct
import zlib
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.request

from browser import DebugSocket
from topology_support import Proxy

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'apps/cli/notemaker'


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def solid_png(red, green, blue):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
    raw=b''.join(b'\0'+bytes((red,green,blue,255))*8 for _ in range(8))
    png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',8,8,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')
    return 'data:image/png;base64,'+base64.b64encode(png).decode()


def read_rgba_png(path):
    source=path.read_bytes();offset=8;compressed=bytearray();width=height=color_type=None
    while offset<len(source):
        length=struct.unpack('>I',source[offset:offset+4])[0];kind=source[offset+4:offset+8]
        data=source[offset+8:offset+8+length];offset+=12+length
        if kind==b'IHDR':width,height,depth,color_type,_,_,_=struct.unpack('>IIBBBBB',data)
        elif kind==b'IDAT':compressed.extend(data)
        elif kind==b'IEND':break
    if depth!=8 or color_type not in (2,6):raise AssertionError('Expected 8-bit RGB/RGBA PNG')
    channels=4 if color_type==6 else 3;stride=width*channels;raw=zlib.decompress(compressed);rows=[];previous=bytearray(stride)
    for y in range(height):
        start=y*(stride+1);filter_type=raw[start];row=bytearray(raw[start+1:start+stride+1])
        for x in range(stride):
            left=row[x-channels] if x>=channels else 0;up=previous[x];upper_left=previous[x-channels] if x>=channels else 0
            if filter_type==1:row[x]=(row[x]+left)&255
            elif filter_type==2:row[x]=(row[x]+up)&255
            elif filter_type==3:row[x]=(row[x]+((left+up)//2))&255
            elif filter_type==4:
                p=left+up-upper_left;pa=abs(p-left);pb=abs(p-up);pc=abs(p-upper_left)
                predictor=left if pa<=pb and pa<=pc else up if pb<=pc else upper_left;row[x]=(row[x]+predictor)&255
            elif filter_type!=0:raise AssertionError('Unknown PNG filter')
        rows.append(row);previous=row
    def pixel(x,y):
        start=x*channels;value=rows[y][start:start+channels]
        return tuple(value)+(255,) if channels==3 else tuple(value)
    return width,height,pixel


class DrawingBrowser(unittest.TestCase):
    def test_cli_synced_shapes_render_as_shapes(self):
        with tempfile.TemporaryDirectory(prefix='notemaker-shape-browser-') as directory:
            out = Path(directory)
            document = 'browser-shapes'
            server_port = free_port()
            server_url = f'http://127.0.0.1:{server_port}'
            server = subprocess.Popen([
                sys.executable, str(ROOT / 'apps/server/server.py'), '--host', '127.0.0.1',
                '--port', str(server_port), '--database', str(out / 'server.sqlite3')
            ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            proxy = None
            browser = None
            browser_log = None
            debug = None
            try:
                for _ in range(100):
                    try:
                        urllib.request.urlopen(server_url + '/api/events?document=' + document, timeout=1).close()
                        break
                    except OSError:
                        time.sleep(.1)
                else:
                    self.fail('NoteMaker server did not start')

                proxy = Proxy(out / 'proxy', document, server_url)

                def cli(*args, value=None):
                    result = subprocess.run([
                        str(CLI), '--proxy-file', str(proxy.descriptor_path), '--document', document,
                        '--actor', 'shape-browser-check', *args
                    ], input=json.dumps(value) if value is not None else None,
                       text=True, capture_output=True, timeout=45)
                    self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
                    return json.loads(result.stdout)

                cli('object', 'put', '-', value={
                    'object': 'triangle', 'shape': 'triangle', 'x': 100, 'y': 100, 'w': 160, 'h': 120,
                    'color': -16777216, 'fill': -65536, 'width': 4
                })
                cli('object', 'put', '-', value={
                    'object': 'star', 'shape': 'star', 'x': 300, 'y': 100, 'w': 160, 'h': 140,
                    'color': -16777216, 'fill': -16711936, 'width': 4
                })
                cli('object', 'put', '-', value={
                    'object': 'signed-line', 'shape': 'line', 'x': 600, 'y': 250, 'w': -160, 'h': 80,
                    'color': -16777216, 'width': 6
                })
                cli('object', 'put', '-', value={
                    'object': 'cube', 'shape': 'cube', 'x': 500, 'y': 100, 'w': 120, 'h': 120,
                    'color': -16777216, 'width': 4, 'fill': -256, 'text': ''
                })
                for index, shape in enumerate(('cube','cylinder','cone','sphere','pyramid')):
                    cli('object', 'put', '-', value={
                        'object': '3d-' + shape, 'shape': shape, 'x': 80 + index*170, 'y': 620, 'w': 100, 'h': 100,
                        'color': -16777216, 'width': 3, 'fill': -256
                    })
                cli('object', 'put', '-', value={
                    'object': 'styled-text', 'shape': 'text', 'x': 500, 'y': 300, 'w': 200, 'h': 70,
                    'color': -16776961, 'width': 2, 'text': 'Styled', 'fontSize': 34,
                    'fontFamily': 'serif', 'bold': True, 'italic': True, 'underline': True,
                    'alignment': 'center'
                })
                cli('object', 'put', '-', value={
                    'object': 'picture', 'shape': 'image', 'x': 700, 'y': 100, 'w': 80, 'h': 80,
                    'color': -16777216, 'width': 1, 'image': solid_png(20, 20, 235)
                })
                cli('object', 'put', '-', value={
                    'object': 'rotated', 'shape': 'rect', 'x': 100, 'y': 400, 'w': 120, 'h': 30,
                    'color': -16777216, 'width': 1, 'fill': -16777216, 'rotation': 90
                })
                cli('object', 'put', '-', value={
                    'object': 'rotated-node', 'shape': 'rect', 'x': 700, 'y': 400, 'w': 100, 'h': 30,
                    'color': -16777216, 'width': 2, 'fill': -1, 'rotation': 45
                })
                cli('object', 'put', '-', value={
                    'object': 'target-node', 'shape': 'rect', 'x': 850, 'y': 450, 'w': 60, 'h': 60,
                    'color': -16777216, 'width': 2, 'fill': -1
                })
                cli('object', 'put', '-', value={
                    'object': 'attached-arrow', 'shape': 'arrow', 'x': 700, 'y': 400, 'w': 210, 'h': 110,
                    'color': -16777216, 'width': 3, 'from': 'rotated-node', 'to': 'target-node',
                    'rotation': 90, 'flipX': True
                })
                cli('object', 'put', '-', value={
                    'object': 'preview-target', 'shape': 'rect', 'x': 100, 'y': 900, 'w': 100, 'h': 50,
                    'color': -16777216, 'width': 2, 'fill': -16777216
                })
                cli_svg = out / 'cli-render.svg'
                cli('erase','-',value=dict(mode='partial',layer='base',radius=8,points=[[740,100],[740,180]]))
                cli_png = out / 'cli-render.png'
                cli('export', str(cli_svg))
                cli('export', str(cli_png))
                svg_text = cli_svg.read_text()
                self.assertIn('font-weight="bold"', svg_text)
                self.assertIn('font-style="italic"', svg_text)
                self.assertIn('text-decoration="underline"', svg_text)
                self.assertIn('rotate(90.0)', svg_text)
                connectors = list(re.finditer(r'<line x1="([^"]+)" y1="([^"]+)" x2="([^"]+)" y2="([^"]+)"', svg_text))
                connector = min(connectors, key=lambda line: abs(float(line.group(1))-792.426))
                self.assertAlmostEqual(float(connector.group(1)), 792.426, delta=.02)
                self.assertAlmostEqual(float(connector.group(2)), 436.213, delta=.02)
                self.assertGreater(cli_png.stat().st_size, 500)
                png_width,png_height,cli_pixel=read_rgba_png(cli_png)
                self.assertEqual((png_width,png_height),(1000,1400))
                self.assertGreater(cli_pixel(720,140)[2],200)
                self.assertGreater(min(cli_pixel(740,140)[:3]),245)
                self.assertGreater(cli_pixel(550,130)[0],200)
                for index in range(5):
                    self.assertGreater(sum(1 for y in range(615,730) for x in range(70+index*170,200+index*170)
                                           if max(cli_pixel(x,y)[:3])<80),20)
                self.assertLess(max(cli_pixel(560,105)[:3]),40)
                self.assertLess(max(cli_pixel(150,460)[:3]),40)
                self.assertGreater(min(cli_pixel(200,410)[:3]),245)
                self.assertGreater(sum(1 for y in range(300,360) for x in range(500,700)
                                       if cli_pixel(x,y)[2]>180 and cli_pixel(x,y)[0]<80),100)
                cli('sync', server_url)
                events = json.load(urllib.request.urlopen(server_url + '/api/events?document=' + document))['events']
                self.assertEqual(len(events), 17)
                preview_entity={'kind':'object','id':'preview-target','page':0}
                cli('selection','select','-',value={'session':'browser-preview','page':0,'entities':[preview_entity]})
                cli('transform','preview','-',value={'session':'browser-preview','entities':[preview_entity],'transform':{'dx':300}})
                local_preview=cli('snapshot')['previews'].get('browser-preview')
                locks=json.load(urllib.request.urlopen(server_url+'/api/locks?document='+document))
                self.assertIn('browser-preview',locks['previews'],{'server':locks,'local':local_preview})

                debug_port = free_port()
                browser_log = (out / 'chromium.log').open('w')
                browser = subprocess.Popen([
                    'chromium', '--headless', '--no-sandbox', '--disable-gpu',
                    f'--remote-debugging-port={debug_port}', '--remote-allow-origins=*',
                    f'--user-data-dir={out / "chromium-profile"}', f'{server_url}/?document={document}'
                ], stdout=browser_log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 40
                last_error = None
                while time.monotonic() < deadline:
                    try:
                        pages = json.load(urllib.request.urlopen(f'http://127.0.0.1:{debug_port}/json', timeout=2))
                        target = next(p for p in pages if p['type'] == 'page' and f'/?document={document}' in p['url'])
                        debug = DebugSocket(target['webSocketDebuggerUrl'])
                        break
                    except Exception as error:
                        last_error = error
                        time.sleep(.25)
                self.assertIsNotNone(debug, f'Chromium debugging connection failed: {last_error}')
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    state = debug.evaluate('({cursor:typeof cursor==="undefined"?null:cursor,count:typeof ops==="undefined"?null:ops.length,status:document.querySelector("#status")?.textContent||"",canvas:!!document.querySelector("canvas")})')
                    if state['cursor'] == 17 and 'Connected' in state['status']:
                        break
                    time.sleep(.2)
                self.assertEqual(state['cursor'], 17, state)
                self.assertIn('Connected', state['status'])

                pixels = debug.evaluate('''(()=>{
                  const p=(x,y)=>Array.from(ctx.getImageData(x,y,1,1).data);
                  const bounds=(test)=>{let d=ctx.getImageData(0,0,1000,1400).data,n=0,minX=1000,minY=1400,maxX=-1,maxY=-1;
                    for(let i=0;i<d.length;i+=4){let x=(i/4)%1000,y=Math.floor(i/4000);if(test(d[i],d[i+1],d[i+2],d[i+3])){n++;minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y)}}
                    return {count:n,bounds:[minX,minY,maxX,maxY]};};
                  return {triangleInterior:p(180,190),triangleCorner:p(103,103),starInterior:p(380,170),starCorner:p(302,102),line:p(520,290),
                    cubeEdge:p(560,105),cubeInterior:p(550,130),threeD:[0,1,2,3,4].map(i=>regionCount(70+i*170,610,200+i*170,735,(r,g,b)=>r<80&&g<80&&b<80)),
                    threeDFill:[0,1,2,3,4].map(i=>regionCount(70+i*170,610,200+i*170,735,(r,g,b)=>r>180&&g>180&&b<120)),
                    picture:p(720,140),pictureHole:p(740,140),styledInk:boundsInText(),rotated:p(150,460),rotationVoid:p(200,410),
                    attachedArrow:p(800,440),unrotatedConnectorGap:p(787,434),
                    previewMoved:p(450,925),previewOriginal:p(150,925),
                    red:bounds((r,g,b,a)=>a>0&&r>220&&g<40&&b<40),green:bounds((r,g,b,a)=>a>0&&g>100&&r<40&&b<40),
                    dark:bounds((r,g,b,a)=>a>0&&r<40&&g<40&&b<40),
                    lineBounds:boundsInLine()};
                  function boundsInLine(){let d=ctx.getImageData(0,0,1000,1400).data,n=0,minX=1000,minY=1400,maxX=-1,maxY=-1;
                    for(let y=245;y<336;y++)for(let x=435;x<606;x++){let i=(y*1000+x)*4;if(d[i]<40&&d[i+1]<40&&d[i+2]<40){n++;minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y)}}
                    return {count:n,bounds:[minX,minY,maxX,maxY]};}
                  function regionCount(x0,y0,x1,y1,test){let d=ctx.getImageData(x0,y0,x1-x0,y1-y0).data,n=0;for(let i=0;i<d.length;i+=4)if(test(d[i],d[i+1],d[i+2]))n++;return n}
                  function boundsInText(){let d=ctx.getImageData(0,0,1000,1400).data,n=0,minX=1000,minY=1400,maxX=-1,maxY=-1;
                    for(let y=300;y<360;y++)for(let x=500;x<700;x++){let i=(y*1000+x)*4;if(d[i]<60&&d[i+1]<60&&d[i+2]>180){n++;minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y)}}
                    return {count:n,bounds:[minX,minY,maxX,maxY]};}
                })()''')
                self.assertGreater(pixels['triangleInterior'][0], 220, pixels)
                self.assertTrue(all(channel>245 for channel in pixels['pictureHole'][:3]),pixels)
                self.assertLess(pixels['triangleInterior'][1], 40, pixels)
                self.assertLess(pixels['triangleInterior'][2], 40, pixels)
                self.assertTrue(all(channel > 245 for channel in pixels['triangleCorner'][:3]), pixels)
                self.assertGreater(pixels['starInterior'][1], 220, pixels)
                self.assertLess(pixels['starInterior'][0], 40, pixels)
                self.assertLess(pixels['starInterior'][2], 40, pixels)
                self.assertTrue(all(channel > 245 for channel in pixels['starCorner'][:3]), pixels)
                self.assertLess(max(pixels['line'][:3]), 40, pixels)
                self.assertLess(max(pixels['cubeEdge'][:3]), 40, pixels)
                self.assertGreater(pixels['cubeInterior'][0],200,pixels)
                self.assertTrue(all(count>20 for count in pixels['threeD']),pixels)
                self.assertTrue(all(count>100 for count in pixels['threeDFill']),pixels)
                self.assertLess(max(pixels['rotated'][:3]),40,pixels)
                self.assertTrue(all(channel>245 for channel in pixels['rotationVoid'][:3]),pixels)
                self.assertLess(max(pixels['attachedArrow'][:3]),40,pixels)
                self.assertTrue(all(channel>245 for channel in pixels['unrotatedConnectorGap'][:3]),pixels)
                self.assertLess(max(pixels['previewMoved'][:3]),40,pixels)
                self.assertTrue(all(channel>245 for channel in pixels['previewOriginal'][:3]),pixels)
                self.assertGreater(pixels['picture'][2], 200, pixels)
                self.assertLess(pixels['picture'][0], 60, pixels)
                self.assertGreater(pixels['styledInk']['count'], 100, pixels)
                self.assertGreater(pixels['red']['count'], 1000, pixels)
                self.assertGreater(pixels['green']['count'], 500, pixels)
                self.assertGreater(pixels['dark']['count'], 100, pixels)
                self.assertGreaterEqual(pixels['red']['bounds'][0], 100, pixels)
                self.assertLessEqual(pixels['red']['bounds'][2], 260, pixels)
                self.assertGreaterEqual(pixels['green']['bounds'][0], 300, pixels)
                self.assertLessEqual(pixels['green']['bounds'][3], 240, pixels)
                self.assertGreater(pixels['lineBounds']['count'], 100, pixels)
                self.assertGreaterEqual(pixels['lineBounds']['bounds'][0], 436, pixels)
                self.assertLessEqual(pixels['lineBounds']['bounds'][0], 440, pixels)
                self.assertGreaterEqual(pixels['lineBounds']['bounds'][2], 600, pixels)
                self.assertLessEqual(pixels['lineBounds']['bounds'][2], 604, pixels)
                self.assertGreaterEqual(pixels['lineBounds']['bounds'][1], 246, pixels)
                self.assertLessEqual(pixels['lineBounds']['bounds'][1], 251, pixels)
                self.assertGreaterEqual(pixels['lineBounds']['bounds'][3], 329, pixels)
                self.assertLessEqual(pixels['lineBounds']['bounds'][3], 334, pixels)
                cli('transform','cancel','browser-preview')
                deadline=time.monotonic()+10
                while time.monotonic()<deadline:
                    reverted=debug.evaluate('({m:Array.from(ctx.getImageData(450,925,1,1).data),o:Array.from(ctx.getImageData(150,925,1,1).data)})')
                    if min(reverted['m'][:3])>245 and max(reverted['o'][:3])<40:break
                    time.sleep(.2)
                self.assertTrue(all(channel>245 for channel in reverted['m'][:3]),reverted)
                self.assertLess(max(reverted['o'][:3]),40,reverted)
                cli('selection','deselect','browser-preview')
                self.assertEqual(len(json.load(urllib.request.urlopen(server_url + '/api/events?document=' + document))['events']),17)
                cli('document','delete')
                cli('sync',server_url)
                deadline=time.monotonic()+10
                while time.monotonic()<deadline:
                    if debug.evaluate("ops.some(o=>o.kind==='document-delete')"):break
                    time.sleep(.2)
                self.assertTrue(debug.evaluate("ops.some(o=>o.kind==='document-delete')"))
                self.assertEqual(debug.evaluate("document.querySelector('#layers').children.length"),0)
                self.assertTrue(debug.evaluate("Array.from(ctx.getImageData(0,100,1000,1300).data).every(v=>v===255)"))
                print('PASS triangle/star fills and signed line render in live server viewer', flush=True)
                print('PASS synced document deletion clears live browser content and layers',flush=True)
            finally:
                if debug:
                    debug.sock.close()
                if browser:
                    browser.terminate()
                    try:
                        browser.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        browser.kill()
                if browser_log:
                    browser_log.close()
                if proxy:
                    proxy.stop()
                server.terminate()
                server.wait(timeout=10)


if __name__ == '__main__':
    unittest.main()

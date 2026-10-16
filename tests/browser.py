"""Headless Chromium check using its debugging protocol and stdlib WebSocket."""
import base64
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
import urllib.request

class DebugSocket:
    def __init__(self, url):
        from urllib.parse import urlparse
        parsed = urlparse(url)
        self.sock = socket.create_connection((parsed.hostname, parsed.port), timeout=15)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(f'GET {parsed.path} HTTP/1.1\r\nHost: {parsed.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n'.encode())
        response = b''
        while not response.endswith(b'\r\n\r\n'):
            response += self.sock.recv(1)
        assert response.split(b'\r\n', 1)[0].split()[1] == b'101', response
        self.counter = 0

    def read(self, size):
        data = b''
        while len(data) < size:
            chunk = self.sock.recv(size-len(data))
            if not chunk:
                raise EOFError()
            data += chunk
        return data

    def request(self, method, params):
        self.counter += 1
        data = json.dumps(dict(id=self.counter, method=method, params=params)).encode()
        mask = os.urandom(4)
        size = len(data)
        header = bytes([0x81, 0x80 | size]) if size < 126 else bytes([0x81, 0x80 | 126]) + struct.pack('!H', size) if size < 65536 else bytes([0x81, 0x80 | 127]) + struct.pack('!Q', size)
        self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i,b in enumerate(data)))
        fragments = b''
        while True:
            first, second = self.read(2)
            length = second & 127
            if length == 126:
                length = struct.unpack('!H', self.read(2))[0]
            elif length == 127:
                length = struct.unpack('!Q', self.read(8))[0]
            payload = self.read(length)
            if first & 15 == 8:
                raise EOFError('WebSocket closed')
            fragments += payload
            if first & 128:
                message = json.loads(fragments)
                fragments = b''
                if message.get('id') == self.counter:
                    assert 'error' not in message, message
                    return message['result']

    def evaluate(self, expression):
        result = self.request('Runtime.evaluate', dict(expression=expression, returnByValue=True))
        assert 'exceptionDetails' not in result, result
        return result['result'].get('value')

def check_browser(out, port, expected_cursor, debug_port=19222, svg_exports=False):
    # Replica protocol 3 uses per-document offsets; the legacy viewer retains
    # SQLite's global sequence cursor. Verify the same committed action count.
    rows=json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/api/events?document=integration'))['events']
    assert len(rows)==expected_cursor, (len(rows),expected_cursor)
    expected_cursor=rows[-1]['seq'] if rows else 0
    # A prior run can leave Chromium's profile lock behind (especially when the
    # host process is interrupted). Containers also share the workspace bind
    # mount, so each concurrent run needs its own profile directory.
    directory = Path(tempfile.mkdtemp(prefix=f'chromium-profile-{debug_port}-', dir=out))
    browser = subprocess.Popen(['chromium', '--headless', '--no-sandbox', '--disable-gpu', f'--remote-debugging-port={debug_port}', '--remote-allow-origins=*', f'--user-data-dir={directory}', f'http://127.0.0.1:{port}/?document=integration'], stdout=(out / 'chromium.log').open('w'), stderr=subprocess.STDOUT)
    debug = None
    try:
        deadline = time.monotonic() + 40
        last_error = None
        while time.monotonic() < deadline:
            try:
                pages = json.load(urllib.request.urlopen(f'http://127.0.0.1:{debug_port}/json', timeout=2))
                target = next(p for p in pages if p['type'] == 'page' and '/?document=integration' in p['url'])
                debug = DebugSocket(target['webSocketDebuggerUrl'])
                break
            except Exception as error:
                last_error = error
                time.sleep(.5)
        assert debug, f'Chromium debugging connection failed: {last_error}'
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            result = debug.evaluate('(()=>{const node=document.querySelector("#status");if(!node)return {cursor:-1,status:""};const status=node.textContent;const match=status.match(/(\\d+) events$/);return {cursor:match?Number(match[1]):-1,status}})()')
            if result.get('cursor') == expected_cursor:
                break
            time.sleep(.5)
        assert result['cursor'] == expected_cursor and 'Connected' in result['status'], result
        if svg_exports:
            deadline=time.monotonic()+20
            while not debug.evaluate('svgImages.size>0&&Array.from(svgImages.values()).every(i=>i.complete&&i.naturalWidth>0)'):
                assert time.monotonic()<deadline,'SVG image did not load'
                time.sleep(.2)
            debug.evaluate('render()')
            pixel=debug.evaluate('Array.from(ctx.getImageData(105,1105,1,1).data)')
            assert max(pixel[:3])<40,pixel
            for name,mime in [('android-cli.png','image/png'),('desktop-svg.png','image/png'),('desktop-svg.svg','image/svg+xml')]:
                data=base64.b64encode((out/name).read_bytes()).decode()
                expression='''new Promise((resolve,reject)=>{let i=new Image();i.onerror=()=>reject('image failed');i.onload=()=>{let c=document.createElement('canvas');c.width=1000;c.height=1400;let g=c.getContext('2d');g.drawImage(i,0,0);resolve(Array.from(g.getImageData(105,1105,1,1).data))};i.src='data:'''+mime+';base64,'+data+"'})"
                result=debug.request('Runtime.evaluate',dict(expression=expression,returnByValue=True,awaitPromise=True))
                assert 'exceptionDetails' not in result,result
                pixel=result['result']['value'];assert max(pixel[:3])<40 and pixel[3]==255,(name,pixel)
            print('PASS SVG pixels in Android/JVM PNG, SVG export, and live browser',flush=True)
        # Force-visible layer must appear checked and disabled even after locally hiding it.
        assert debug.evaluate('hidden.add("test-layer");render();Array.from(document.querySelectorAll("#layers input")).some(x=>x.disabled&&x.checked)')
        png = debug.evaluate('canvas.toDataURL("image/png")')
        (out / 'browser-canvas.png').write_bytes(base64.b64decode(png.split(',')[1]))
        stats = debug.evaluate('(()=>{let p=ctx.getImageData(0,0,1000,1400).data,n=0,minX=1000,minY=1400,maxX=-1,maxY=-1;for(let i=0;i<p.length;i+=4)if(p[i+3]>0&&(p[i]<245||p[i+1]<245||p[i+2]<245)){n++;let x=(i/4)%1000,y=Math.floor(i/4000);minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y)}return {pixels:n,bounds:[minX,minY,maxX,maxY]}})()')
        assert stats['pixels'] > 1000, stats
        (out / 'browser-stats.json').write_text(json.dumps(stats))
        return stats
    finally:
        if debug:
            debug.sock.close()
        browser.terminate()
        try:
            browser.wait(timeout=10)
        except subprocess.TimeoutExpired:
            browser.kill()

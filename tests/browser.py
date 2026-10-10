"""Headless Chromium check using its debugging protocol and stdlib WebSocket."""
import base64
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
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

def check_browser(out, port, expected_cursor):
    directory = out / 'chromium-profile'
    browser = subprocess.Popen(['chromium', '--headless', '--no-sandbox', '--disable-gpu', '--remote-debugging-port=19222', '--remote-allow-origins=*', f'--user-data-dir={directory}', f'http://127.0.0.1:{port}/?document=integration'], stdout=(out / 'chromium.log').open('w'), stderr=subprocess.STDOUT)
    debug = None
    try:
        deadline = time.monotonic() + 40
        last_error = None
        while time.monotonic() < deadline:
            try:
                pages = json.load(urllib.request.urlopen('http://127.0.0.1:19222/json', timeout=2))
                target = next(p for p in pages if p['type'] == 'page' and '/?document=integration' in p['url'])
                debug = DebugSocket(target['webSocketDebuggerUrl'])
                break
            except Exception as error:
                last_error = error
                time.sleep(.5)
        assert debug, f'Chromium debugging connection failed: {last_error}'
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            result = debug.evaluate('({cursor, count:ops.length, status:document.querySelector("#status").textContent})')
            if result.get('cursor') == expected_cursor:
                break
            time.sleep(.5)
        assert result['cursor'] == expected_cursor and 'Connected' in result['status'], result
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

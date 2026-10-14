"""Real processes and controllable network links for replica property tests."""
import json
import socket
import subprocess
import threading
import time
from urllib.parse import parse_qs, urlparse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'apps/cli/notemaker'

def http(url, body=None, token=None, timeout=8):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    with urllib.request.urlopen(urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(), headers=headers), timeout=timeout) as response:
        return json.load(response)

class Proxy:
    def __init__(self, directory, document='property', upstream=None):
        self.directory = Path(directory)
        self.document = document
        self.port = 0
        self.process = None
        self.start(upstream)

    def start(self, upstream=None):
        args = [str(CLI), '--state', str(self.directory), '--document', self.document, 'serve', str(self.port)]
        if upstream is not None:
            args.append(str(upstream))
        self.process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        ready = self.process.stdout.readline()
        if not ready:
            raise AssertionError(self.process.stderr.read())
        self.port = json.loads(ready)['port']
        self.descriptor_path = self.directory / 'proxy.json'
        self.descriptor = json.loads(self.descriptor_path.read_text())

    def command(self, *args, value=None, actor='agent'):
        return http(self.descriptor['url']+'/command', dict(document=self.document, actor=actor, args=list(args), input=value), self.descriptor['token'], timeout=40)['result']

    def cli(self, *args, value=None, actor='agent', direct=False, ok=True):
        flags = ['--state', str(self.directory)] if direct else ['--proxy-file', str(self.descriptor_path)]
        result = subprocess.run([str(CLI), *flags, '--document', self.document, '--actor', actor, *args], input=json.dumps(value) if value is not None else None, text=True, capture_output=True, timeout=45)
        assert (result.returncode == 0) == ok, (result.stdout, result.stderr)
        return json.loads(result.stdout if ok else result.stderr)

    def stop(self, crash=False):
        if self.process and self.process.poll() is None:
            (self.process.kill if crash else self.process.terminate)()
            self.process.wait(timeout=10)
        if self.process:
            self.process.stdout.close(); self.process.stderr.close()

class Link:
    """Fault injection only: forwards the production protocol, including SSE."""
    def __init__(self, upstream, descriptor_path, token=None):
        self.upstream, self.token = upstream, token
        self.online = True
        self.duplicate = False
        self.drop = 0
        self.uploads = []
        self.reads = []
        self.connections = set()
        self.counter_lock=threading.Lock()
        self.active_uploads=0
        self.max_active_uploads=0
        self.delayed_document=None
        self.delay_seconds=0
        self.delay_started=threading.Event()
        self.delay_used=False
        link = self
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self): self.forward()
            def do_POST(self): self.forward()
            def log_message(self, *args): pass
            def forward(self):
                uploading=False;event_upload=self.command=='POST' and self.path.split('?',1)[0]=='/api/events'
                try:
                    if not link.online:
                        self.send_error(503); return
                    data = self.rfile.read(int(self.headers['Content-Length'])) if self.command == 'POST' else None
                    if self.command=='GET' and self.path.split('?',1)[0]=='/api/replica':
                        doc=parse_qs(urlparse(self.path).query).get('document',['notes'])[0]
                        with link.counter_lock:
                            delay=doc==link.delayed_document and not link.delay_used
                            if delay:link.delay_used=True
                        if delay:
                            link.delay_started.set();time.sleep(link.delay_seconds)
                    headers = {'Content-Type': 'application/json'}
                    if link.token: headers['Authorization'] = 'Bearer '+link.token
                    request = urllib.request.Request(link.upstream+self.path, data=data, headers=headers)
                    if event_upload:
                        with link.counter_lock:
                            uploading=True;link.active_uploads+=1
                            link.max_active_uploads=max(link.max_active_uploads,link.active_uploads)
                        link.uploads.append((time.monotonic(), json.loads(data), len(data)))
                        if link.duplicate:
                            urllib.request.urlopen(request, timeout=8).close()
                    elif self.command=='GET' and self.path.split('?',1)[0]=='/api/events':
                        link.reads.append(self.path)
                    with urllib.request.urlopen(request, timeout=25) as response:
                        if event_upload and link.drop:
                            link.drop -= 1; self.close_connection = True; return
                        self.send_response(200); self.send_header('Content-Type', response.headers.get('Content-Type')); self.end_headers()
                        if self.path.startswith('/api/stream'):
                            link.connections.add(self.connection)
                            while link.online:
                                line = response.readline()
                                if not line: break
                                self.wfile.write(line); self.wfile.flush()
                        else:
                            self.wfile.write(response.read())
                except (OSError, ValueError): pass
                finally:
                    if uploading:
                        with link.counter_lock: link.active_uploads-=1
                    link.connections.discard(self.connection)
                    self.close_connection = True
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True); self.thread.start()
        self.url = f'http://127.0.0.1:{self.server.server_port}'
        self.path = Path(descriptor_path)
        # Token also identifies this as a proxy edge for recursive cycle checks.
        self.path.write_text(json.dumps(dict(url=self.url, token=token))) if token else None

    def delay_next_replica(self,document,seconds):
        with self.counter_lock:
            self.delayed_document=document;self.delay_seconds=seconds;self.delay_used=False;self.delay_started.clear()

    @property
    def target(self): return str(self.path) if self.token else self.url

    def disconnect(self):
        self.online = False
        for connection in list(self.connections):
            try: connection.shutdown(socket.SHUT_RDWR)
            except OSError: pass

    def close(self):
        self.disconnect(); self.server.shutdown(); self.server.server_close(); self.thread.join()

def eventually(check, timeout=25):
    deadline = time.monotonic()+timeout
    while True:
        try:
            result = check()
            if result: return result
        except (OSError, AssertionError):
            if time.monotonic() >= deadline: raise
        if time.monotonic() >= deadline: raise AssertionError('Replica convergence timed out')
        time.sleep(.05)

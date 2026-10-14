#!/usr/bin/env python3
"""Dependency-free, durable document operation log and LAN viewer."""
import argparse
import json
import sqlite3
import threading
import math
import uuid
import time
import signal
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs
from svg_asset import validate_svg
from agent.chats import Chats, ChatError

LOCK = threading.Lock()
CHANGES = threading.Condition(LOCK)
GENERATION = 0
DB = None
CHATS = None

def validate(op, nested=False):
    if not isinstance(op, dict) or not isinstance(op.get('id'), str) or len(op['id']) > 200:
        raise ValueError('Operation needs an id')
    kind = op.get('kind')
    if kind == 'batch':
        actions = op.get('actions')
        if nested or not isinstance(actions, list) or not 1 <= len(actions) <= 200:
            raise ValueError('Invalid or nested action batch')
        for action in actions:
            if not isinstance(action, dict):
                raise ValueError('Invalid action')
            validate(dict(action, id='nested'), nested=True)
    elif kind == 'object':
        if not all(isinstance(op.get(k), str) and 0 < len(op[k]) <= 200 for k in ('object', 'layer')):
            raise ValueError('Invalid object identity')
        if op.get('shape') not in ('rect', 'ellipse', 'text', 'arrow', 'svg'):
            raise ValueError('Invalid shape')
        if op['shape'] == 'svg':
            validate_svg(op.get('svg'))
        for k in ('x', 'y', 'w', 'h', 'width', 'page', 'color'):
            if type(op.get(k)) not in (int, float) or not math.isfinite(op[k]):
                raise ValueError('Invalid object number: ' + k)
        if any(abs(op[k]) > 1000000 for k in ('x', 'y', 'w', 'h')) or not 1 <= op['width'] <= 100:
            raise ValueError('Object geometry out of range')
        if op['shape'] != 'arrow' and (op['w'] <= 0 or op['h'] <= 0):
            raise ValueError('Object dimensions must be positive')
        if not 0 <= op['page'] <= 10000 or op['page'] != int(op['page']):
            raise ValueError('Invalid page')
        for k in ('color', 'fill'):
            if k in op and (type(op[k]) not in (int, float) or not math.isfinite(op[k]) or not -2147483648 <= op[k] <= 2147483647 or op[k] != int(op[k])):
                raise ValueError('Invalid color')
        if 'text' in op and (not isinstance(op['text'], str) or len(op['text']) > 10000 or type(op.get('fontSize')) not in (int, float) or not 1 <= op['fontSize'] <= 500):
            raise ValueError('Invalid object text')
        for k in ('from', 'to'):
            if k in op and (not isinstance(op[k], str) or not 0 < len(op[k]) <= 200):
                raise ValueError('Invalid connector')
    elif kind in ('object-active', 'layer-active'):
        key = 'object' if kind == 'object-active' else 'layer'
        if not isinstance(op.get(key), str) or not isinstance(op.get('active'), bool) or (key == 'layer' and op[key] == 'base'):
            raise ValueError('Invalid visibility')
    elif kind == 'document':
        if op.get('type') not in ('fixed', 'pages', 'infinite'):
            raise ValueError('Unknown document type')
    elif kind == 'layer':
        if not isinstance(op.get('layer'), str) or not isinstance(op.get('name'), str) or not isinstance(op.get('force'), bool):
            raise ValueError('Invalid layer')
    elif kind == 'stroke':
        if not isinstance(op.get('stroke'), str) or not isinstance(op.get('layer'), str):
            raise ValueError('Invalid stroke')
        if not isinstance(op.get('page'), int) or op['page'] < 0 or op['page'] > 10000:
            raise ValueError('Invalid page')
        if not isinstance(op.get('color'), int) or not 1 <= op.get('width', 0) <= 100:
            raise ValueError('Invalid brush')
        points = op.get('points')
        if not isinstance(points, list) or not 1 <= len(points) <= 10000:
            raise ValueError('Invalid points')
        for p in points:
            if len(p) != 3 or any(not isinstance(n, (float, int)) or not -1000000 <= n <= 1000000 for n in p) or not 0 <= p[2] <= 1:
                raise ValueError('Invalid point')
    elif kind == 'active':
        if not isinstance(op.get('stroke'), str) or not isinstance(op.get('active'), bool):
            raise ValueError('Invalid stroke visibility')
    else:
        raise ValueError('Unknown operation')

class Handler(BaseHTTPRequestHandler):
    def chat_request(self):
        url = urlparse(self.path)
        if not (url.path == '/api/chats' or url.path.startswith('/api/chats/')):
            return False
        try:
            parts = url.path.strip('/').split('/')
            q = parse_qs(url.query)
            if self.command == 'GET':
                if parts == ['api', 'chats', 'options']:
                    self.reply(200, CHATS.options())
                elif len(parts) == 2:
                    self.reply(200, CHATS.list(int(q.get('limit', ['5'])[0]), int(q.get('offset', ['0'])[0])))
                elif len(parts) == 3:
                    self.reply(200, CHATS.get(parts[2]))
                elif len(parts) == 4 and parts[3] in ('events', 'stream'):
                    after = int(q.get('after', ['0'])[0])
                    events = CHATS.events(parts[2], after)
                    if parts[3] == 'events':
                        self.reply(200, {'events': events})
                    else:
                        self.send_response(200)
                        self.send_header('Content-Type', 'application/x-ndjson; charset=utf-8')
                        self.send_header('Cache-Control', 'no-store')
                        self.send_header('Connection', 'close')
                        self.end_headers()
                        self.connection.settimeout(10)
                        deadline = time.monotonic() + 20
                        while True:
                            for event in events:
                                self.wfile.write((json.dumps(event) + '\n').encode())
                                after = event['seq']
                            self.wfile.write(b'{}\n')  # heartbeat, also flush HTTP headers immediately
                            self.wfile.flush()
                            if time.monotonic() >= deadline or CHATS.closed:
                                break
                            events = CHATS.events(parts[2], after, wait=2)
                        self.close_connection = True
                else:
                    raise ChatError('Not found', 404)
            elif self.command == 'POST':
                # Prevent ordinary browser forms and cross-origin pages starting agents.
                origin = self.headers.get('Origin')
                if origin and urlparse(origin).netloc != self.headers.get('Host'):
                    raise ChatError('Cross-origin chat request rejected', 403)
                if self.headers.get('Content-Type', '').split(';')[0] != 'application/json':
                    raise ChatError('Expected application/json', 415)
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 150000:
                    raise ChatError('Invalid request size')
                body = json.loads(self.rfile.read(length))
                if not isinstance(body, dict):
                    raise ChatError('Expected an object')
                if len(parts) == 2:
                    self.reply(200, CHATS.create(body))
                elif len(parts) == 4 and parts[3] == 'messages':
                    with LOCK:
                        documents = [row[0] for row in DB.execute('SELECT DISTINCT document FROM events ORDER BY document')]
                    self.reply(200, CHATS.send(parts[2], body, documents))
                elif len(parts) == 4 and parts[3] == 'stop':
                    self.reply(200, CHATS.stop(parts[2]))
                else:
                    raise ChatError('Not found', 404)
        except (ValueError, KeyError, TypeError) as error:
            self.reply(getattr(error, 'status', 400), {'error': str(error)})
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            pass
        return True

    def reply(self, status, value, mime='application/json'):
        data = (json.dumps(value) if mime == 'application/json' else value).encode()
        self.send_response(status)
        self.send_header('Content-Type', mime)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.chat_request():
            return
        url = urlparse(self.path)
        if url.path == '/':
            return self.reply(200, Path(__file__).with_name('viewer.html').read_text(), 'text/html; charset=utf-8')
        if url.path == '/api/info':
            with LOCK:
                identity = DB.execute("SELECT value FROM metadata WHERE key='id'").fetchone()[0]
                documents = [r[0] for r in DB.execute('SELECT DISTINCT document FROM events')]
            return self.reply(200, {'id': identity, 'rootId': identity, 'kind': 'server', 'protocol': 3, 'lineage': [], 'documents': documents})
        if url.path == '/api/stream':
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Connection', 'close')
            self.end_headers()
            self.connection.settimeout(20)
            seen = -1
            try:
                while True:
                    with CHANGES:
                        if seen == GENERATION:
                            CHANGES.wait(timeout=15)
                        current = GENERATION
                    # Always wake on connection: durable cursors, not notifications,
                    # determine catch-up. Heartbeats do not cause document reads.
                    message = f'data: {current}\n\n' if current != seen else ': heartbeat\n\n'
                    self.wfile.write(message.encode()); self.wfile.flush()
                    seen = current
            except (OSError, TimeoutError):
                pass
            self.close_connection = True
            return
        if url.path == '/api/replica':
            q = parse_qs(url.query)
            try:
                doc, after = q.get('document', ['notes'])[0], int(q.get('after', ['0'])[0])
                if after < 0:
                    raise ValueError('Invalid cursor')
                with LOCK:
                    identity = DB.execute("SELECT value FROM metadata WHERE key='id'").fetchone()[0]
                    count = DB.execute('SELECT count(*) FROM events WHERE document=?', (doc,)).fetchone()[0]
                    if after > count:
                        raise ValueError('Invalid cursor')
                    rows = DB.execute('SELECT payload FROM events WHERE document=? ORDER BY seq LIMIT 100 OFFSET ?', (doc, after)).fetchall()
                events, size = [], 0
                for (payload,) in rows:
                    length = len(payload.encode())
                    if events and size + length > 3_000_000:
                        break
                    events.append(json.loads(payload)); size += length
                return self.reply(200, dict(events=events, after=after+len(events), more=after+len(events)<count,
                                            provisional=[], pendingAfter=0, pendingMore=False, rootId=identity))
            except ValueError as error:
                return self.reply(400, {'error': str(error)})
        if url.path == '/api/events':
            q = parse_qs(url.query)
            try:
                doc = q.get('document', ['notes'])[0]
                after = int(q.get('after', ['0'])[0])
                with LOCK:
                    rows = DB.execute('SELECT seq, payload FROM events WHERE document=? AND seq>? ORDER BY seq LIMIT 1000', (doc, after)).fetchall()
                return self.reply(200, {'events': [dict(seq=s, op=json.loads(p)) for s, p in rows]})
            except ValueError:
                return self.reply(400, {'error': 'Invalid cursor'})
        self.reply(404, {'error': 'Not found'})

    def do_POST(self):
        global GENERATION
        if self.chat_request():
            return
        if urlparse(self.path).path != '/api/events':
            return self.reply(404, {'error': 'Not found'})
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 4_000_000:
                raise ValueError('Request too large')
            body = json.loads(self.rfile.read(length))
            doc, ops = body['document'], body['operations']
            if not isinstance(doc, str) or not 1 <= len(doc) <= 200 or not isinstance(ops, list) or len(ops) > 200:
                raise ValueError('Invalid document or batch')
            for op in ops:
                validate(op)
            with LOCK, DB:
                identity = DB.execute("SELECT value FROM metadata WHERE key='id'").fetchone()[0]
                if body.get('rootId') is not None and body['rootId'] != identity:
                    raise ValueError('Server database differs from this replica')
                for op in ops:
                    existing = DB.execute('SELECT payload FROM events WHERE document=? AND operation=?', (doc, op['id'])).fetchone()
                    payload = json.dumps(op, sort_keys=True)
                    if existing and existing[0] != payload:
                        raise ValueError('Operation id reused with different contents')
                    DB.execute('INSERT OR IGNORE INTO events(document, operation, payload) VALUES(?,?,?)', (doc, op['id'], payload))
            with CHANGES:
                GENERATION += 1
                CHANGES.notify_all()
            self.reply(200, {'accepted': [op['id'] for op in ops]})
        except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
            self.reply(400, {'error': str(e)})

    def log_message(self, fmt, *args):
        if args and str(args[1]) != '200':
            super().log_message(fmt, *args)

def main():
    global DB, CHATS
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', default='0.0.0.0')
    parser.add_argument('--port', type=int, default=8765)
    parser.add_argument('--database', default='notemaker.sqlite3')
    parser.add_argument('--agent-root', default=str(Path(__file__).resolve().parents[2]))
    parser.add_argument('--agent-data', help='Private chat sessions and CLI replica; default: DATABASE parent/.agent-data')
    parser.add_argument('--pi-command', default='pi')
    parser.add_argument('--agent-upstream', help='CLI sync URL; default: http://127.0.0.1:PORT')
    parser.add_argument('--chat-models', help='JSON file containing provider, model and efforts options')
    args = parser.parse_args()
    DB = sqlite3.connect(args.database, check_same_thread=False)
    DB.execute('PRAGMA journal_mode=WAL')
    DB.execute('CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, document TEXT NOT NULL, operation TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(document, operation))')
    DB.execute('CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL)')
    DB.execute("INSERT OR IGNORE INTO metadata VALUES('id', ?)", (str(uuid.uuid4()),))
    DB.commit()
    CHATS = Chats(args.database, args.agent_root,
                  args.agent_data or str(Path(args.database).resolve().parent / '.agent-data'),
                  args.pi_command, args.agent_upstream or f'http://127.0.0.1:{args.port}',
                  json.loads(Path(args.chat_models).read_text()) if args.chat_models else None)
    print(f'NoteMaker listening on http://{args.host}:{args.port}', flush=True)
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    def stop(signum, frame):
        raise SystemExit(0)
    signal.signal(signal.SIGTERM, stop)
    try:
        server.serve_forever()
    finally:
        CHATS.close()
        server.server_close()

if __name__ == '__main__':
    main()

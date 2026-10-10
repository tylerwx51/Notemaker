#!/usr/bin/env python3
"""Dependency-free, durable document operation log and LAN viewer."""
import argparse
import json
import sqlite3
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

LOCK = threading.Lock()
DB = None

def validate(op):
    if not isinstance(op, dict) or not isinstance(op.get('id'), str) or len(op['id']) > 200:
        raise ValueError('Operation needs an id')
    kind = op.get('kind')
    if kind == 'document':
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
    def reply(self, status, value, mime='application/json'):
        data = (json.dumps(value) if mime == 'application/json' else value).encode()
        self.send_response(status)
        self.send_header('Content-Type', mime)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        url = urlparse(self.path)
        if url.path == '/':
            return self.reply(200, Path(__file__).with_name('viewer.html').read_text(), 'text/html; charset=utf-8')
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
                for op in ops:
                    existing = DB.execute('SELECT payload FROM events WHERE document=? AND operation=?', (doc, op['id'])).fetchone()
                    payload = json.dumps(op, sort_keys=True)
                    if existing and existing[0] != payload:
                        raise ValueError('Operation id reused with different contents')
                    DB.execute('INSERT OR IGNORE INTO events(document, operation, payload) VALUES(?,?,?)', (doc, op['id'], payload))
            self.reply(200, {'accepted': [op['id'] for op in ops]})
        except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
            self.reply(400, {'error': str(e)})

    def log_message(self, fmt, *args):
        if args and str(args[1]) != '200':
            super().log_message(fmt, *args)

def main():
    global DB
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', default='0.0.0.0')
    parser.add_argument('--port', type=int, default=8765)
    parser.add_argument('--database', default='notemaker.sqlite3')
    args = parser.parse_args()
    DB = sqlite3.connect(args.database, check_same_thread=False)
    DB.execute('PRAGMA journal_mode=WAL')
    DB.execute('CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, document TEXT NOT NULL, operation TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(document, operation))')
    DB.commit()
    print(f'NoteMaker listening on http://{args.host}:{args.port}', flush=True)
    ThreadingHTTPServer((args.host, args.port), Handler).serve_forever()

if __name__ == '__main__':
    main()

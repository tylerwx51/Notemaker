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
import base64
import struct
import zlib
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
PREVIEWS = {}

def validate(op, nested=False):
    if not isinstance(op, dict) or not isinstance(op.get('id'), str) or len(op['id']) > 200:
        raise ValueError('Operation needs an id')
    for key in ('actor','session'):
        if key in op and (not isinstance(op[key],str) or not 0<len(op[key])<=200): raise ValueError('Invalid '+key)
    kind = op.get('kind')
    if kind in ('stroke','object') and 'erasures' in op:
        masks=op['erasures']
        if not isinstance(masks,list) or len(masks)>2000: raise ValueError('Invalid erasure mask')
        count=0
        for polygon in masks:
            if not isinstance(polygon,list) or not 3<=len(polygon)<=4096: raise ValueError('Invalid erasure region')
            count+=len(polygon)
            if count>64000: raise ValueError('Erasure mask too complex')
            for point in polygon:
                if not isinstance(point,list) or len(point)!=2 or any(type(v) not in (int,float) or not math.isfinite(v) or abs(v)>1e9 for v in point): raise ValueError('Invalid erasure vertex')

    if kind == 'batch':
        actions = op.get('actions')
        if nested or not isinstance(actions, list) or not 1 <= len(actions) <= 200:
            raise ValueError('Invalid or nested action batch')
        for action in actions:
            if not isinstance(action, dict):
                raise ValueError('Invalid action')
            validate(dict(action, id='nested'), nested=True)
        if 'bases' in op:
            bases=op['bases']
            if not isinstance(bases,dict) or len(bases)>200 or any(not isinstance(k,str) or len(k)>220 or not isinstance(v,str) or len(v)>200 for k,v in bases.items()): raise ValueError('Invalid base versions')
        if 'visibility' in op:
            visibility=op['visibility']
            if not isinstance(visibility,dict) or len(visibility)>200 or any(not isinstance(k,str) or len(k)>220 or type(v) is not bool for k,v in visibility.items()): raise ValueError('Invalid branch visibility')
    elif kind == 'object':
        if not all(isinstance(op.get(k), str) and 0 < len(op[k]) <= 200 for k in ('object', 'layer')):
            raise ValueError('Invalid object identity')
        if op.get('shape') not in ('rect', 'ellipse', 'text', 'line', 'triangle', 'diamond', 'pentagon', 'hexagon', 'star', 'arrow', 'svg', 'image', 'cube', 'cylinder', 'cone', 'sphere', 'pyramid', 'trapezoid', 'triangle45', 'triangle60', 'triangle90', 'wave', 'curve', 'polyline', 'regularPolygon', 'circle', 'rectangle', 'rhombus', 'arrowLine', 'waveLine', 'regularHexagon', 'template'):
            raise ValueError('Invalid shape')
        if op['shape'] == 'template':
            if op.get('template') not in ('ruled', 'grid', 'dots'):
                raise ValueError('Invalid infinite template')
            spacing=op.get('spacing')
            if type(spacing) not in (int,float) or not math.isfinite(spacing) or not 8 <= spacing <= 1000000:
                raise ValueError('Invalid template spacing')
        if op['shape'] == 'svg':
            validate_svg(op.get('svg'))
        if op['shape'] == 'image':
            uri=op.get('image','')
            if not isinstance(uri,str) or len(uri)>2_800_000 or not (uri.startswith('data:image/png;base64,') or uri.startswith('data:image/jpeg;base64,')): raise ValueError('Invalid image data URI')
            try: data=base64.b64decode(uri.split(',',1)[1],validate=True)
            except Exception: raise ValueError('Invalid image base64')
            if not data or len(data)>2*1024*1024: raise ValueError('Image exceeds 2 MiB')
            if uri.startswith('data:image/png'):
                if len(data)<24 or data[:8]!=b'\x89PNG\r\n\x1a\n' or data[8:12]!=b'\x00\x00\x00\r' or data[12:16]!=b'IHDR': raise ValueError('Invalid PNG header')
                iw,ih=struct.unpack('>II',data[16:24])
                offset=8; ended=False; pixels=False
                while offset+12<=len(data):
                    size=struct.unpack('>I',data[offset:offset+4])[0];end=offset+12+size
                    if end>len(data): raise ValueError('Truncated PNG chunk')
                    kind=data[offset+4:offset+8];chunk=data[offset+4:offset+8+size];crc=struct.unpack('>I',data[offset+8+size:end])[0]
                    if (zlib.crc32(chunk)&0xffffffff)!=crc: raise ValueError('Invalid PNG chunk checksum')
                    offset=end
                    if kind==b'IDAT': pixels=True
                    if kind==b'IEND':
                        if size!=0: raise ValueError('Invalid PNG end')
                        ended=True;break
                if not ended or not pixels or offset!=len(data): raise ValueError('Incomplete PNG image')
            else:
                if len(data)<4 or data[:2]!=b'\xff\xd8': raise ValueError('Invalid JPEG header')
                if data[-2:]!=b'\xff\xd9': raise ValueError('Truncated JPEG image')
                iw=ih=0; p=2
                while p+3<len(data):
                    if data[p]!=255: raise ValueError('Invalid JPEG marker')
                    while p<len(data) and data[p]==255: p+=1
                    if p>=len(data): break
                    marker=data[p];p+=1
                    if marker in (217,218): break
                    if marker==1 or 208<=marker<=215: continue
                    if p+1>=len(data): break
                    size=struct.unpack('>H',data[p:p+2])[0]
                    if size<2 or p+size>len(data): raise ValueError('Invalid JPEG segment')
                    if 192<=marker<=207 and marker not in (196,200,204):
                        if size<8: raise ValueError('Invalid JPEG frame')
                        ih,iw=struct.unpack('>HH',data[p+3:p+7]);break
                    p+=size
            if not 0<iw<=8192 or not 0<ih<=8192 or iw*ih>32*1024*1024: raise ValueError('Invalid image dimensions')
        for k in ('x', 'y', 'w', 'h', 'width', 'page', 'color'):
            if type(op.get(k)) not in (int, float) or not math.isfinite(op[k]):
                raise ValueError('Invalid object number: ' + k)
        if any(abs(op[k]) > 1000000 for k in ('x', 'y', 'w', 'h')) or not 1 <= op['width'] <= 100:
            raise ValueError('Object geometry out of range')
        if op['shape'] not in ('arrow', 'line', 'arrowLine') and (op['w'] <= 0 or op['h'] <= 0):
            raise ValueError('Object dimensions must be positive')
        if not 0 <= op['page'] <= 10000 or op['page'] != int(op['page']):
            raise ValueError('Invalid page')
        for k in ('color', 'fill'):
            if k in op and (type(op[k]) not in (int, float) or not math.isfinite(op[k]) or not -2147483648 <= op[k] <= 2147483647 or op[k] != int(op[k])):
                raise ValueError('Invalid color')
        if 'text' in op and (not isinstance(op['text'], str) or len(op['text']) > 10000 or type(op.get('fontSize')) not in (int, float) or not 1 <= op['fontSize'] <= 500):
            raise ValueError('Invalid object text')
        for k in ('bold','italic','underline'):
            if k in op and type(op[k]) is not bool: raise ValueError('Invalid text style')
        if 'fontFamily' in op and op['fontFamily'] not in ('sans-serif','serif','monospace'): raise ValueError('Invalid font family')
        if 'alignment' in op and op['alignment'] not in ('left','center','right'): raise ValueError('Invalid text alignment')
        if 'rotation' in op and (type(op['rotation']) not in (int,float) or not math.isfinite(op['rotation']) or abs(op['rotation'])>360): raise ValueError('Invalid object rotation')
        for k in ('flipX','flipY'):
            if k in op and type(op[k]) is not bool: raise ValueError('Invalid object flip')
        for k in ('from', 'to'):
            if k in op and (not isinstance(op[k], str) or not 0 < len(op[k]) <= 200):
                raise ValueError('Invalid connector')
    elif kind in ('object-active', 'layer-active'):
        key = 'object' if kind == 'object-active' else 'layer'
        if not isinstance(op.get(key), str) or not isinstance(op.get('active'), bool) or (key == 'layer' and op[key] == 'base'):
            raise ValueError('Invalid visibility')
    elif kind == 'document-delete':
        if nested:
            raise ValueError('Document deletion must be a standalone action')
    elif kind == 'document':
        if op.get('type') not in ('fixed', 'pages', 'infinite'):
            raise ValueError('Unknown document type')
    elif kind == 'layer':
        if not isinstance(op.get('layer'), str) or not isinstance(op.get('name'), str) or not isinstance(op.get('force'), bool):
            raise ValueError('Invalid layer')
        if 'order' in op and (type(op['order']) not in (int,float) or not math.isfinite(op['order']) or abs(op['order'])>1_000_000): raise ValueError('Invalid layer order')
    elif kind == 'stroke':
        if not isinstance(op.get('stroke'), str) or not isinstance(op.get('layer'), str):
            raise ValueError('Invalid stroke')
        if not isinstance(op.get('page'), int) or op['page'] < 0 or op['page'] > 10000:
            raise ValueError('Invalid page')
        if not isinstance(op.get('color'), int) or not 1 <= op.get('width', 0) <= 100:
            raise ValueError('Invalid brush')
        if 'brush' in op and op['brush'] not in ('pen','pencil','marker','brush','oily','fountain','charcoal','neoBrush','latinCalligraphy','asiaCalligraphy','square'): raise ValueError('Unknown brush')
        if 'material' in op and (type(op['material']) is not int or not 0 <= op['material'] <= 100): raise ValueError('Invalid material percentage')
        points = op.get('points')
        if not isinstance(points, list) or not 1 <= len(points) <= 10000:
            raise ValueError('Invalid points')
        for p in points:
            if len(p) != 3 or any(not isinstance(n, (float, int)) or not -1000000 <= n <= 1000000 for n in p) or not 0 <= p[2] <= 1:
                raise ValueError('Invalid point')
    elif kind == 'active':
        if not isinstance(op.get('stroke'), str) or not isinstance(op.get('active'), bool):
            raise ValueError('Invalid stroke visibility')
    elif kind == 'conflict-status':
        if not isinstance(op.get('conflict'),str) or not 0<len(op['conflict'])<=200 or not isinstance(op.get('active'),bool): raise ValueError('Invalid conflict status')
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
                        documents = [row[0] for row in DB.execute('SELECT DISTINCT document FROM events WHERE document NOT IN (SELECT document FROM deleted_documents) ORDER BY document')]
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
                documents = [r[0] for r in DB.execute('SELECT DISTINCT document FROM events WHERE document NOT IN (SELECT document FROM deleted_documents)')]
                deleted = [r[0] for r in DB.execute('SELECT document FROM deleted_documents ORDER BY document')]
            return self.reply(200, {'id': identity, 'rootId': identity, 'kind': 'server', 'protocol': 3, 'lineage': [], 'documents': documents, 'deletedDocuments': deleted})
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
        if url.path == '/api/locks':
            q = parse_qs(url.query); doc = q.get('document', ['notes'])[0]; now = int(time.time() * 1000)
            with LOCK, DB:
                DB.execute('DELETE FROM leases WHERE expires<=?', (now,))
                rows = DB.execute('SELECT entity, payload FROM leases WHERE document=?', (doc,)).fetchall()
                leases={key:json.loads(payload) for key,payload in rows}
                for key,preview in list(PREVIEWS.items()):
                    if key[0]!=doc: continue
                    valid=preview.get('expires',0)>now and isinstance(preview.get('entities'),list) and bool(preview['entities'])
                    limit=preview.get('expires',0)
                    for entity in preview.get('entities',[]):
                        ref=f"{int(entity.get('page',0))}:{entity.get('kind')}:{entity.get('id')}";lease=leases.get(ref,{})
                        if lease.get('actor')!=preview.get('actor') or lease.get('session')!=preview.get('session') or lease.get('expires',0)<=now: valid=False;break
                        limit=min(limit,lease['expires'])
                    if not valid or preview.get('expires',0)>limit: PREVIEWS.pop(key,None)
                previews = {session: preview for (document,session),preview in PREVIEWS.items() if document==doc}
            return self.reply(200, {'leases': {key: json.loads(payload) for key, payload in rows},
                                    'previews': previews})
        self.reply(404, {'error': 'Not found'})

    def do_POST(self):
        global GENERATION
        if self.chat_request():
            return
        if urlparse(self.path).path == '/api/locks':
            global GENERATION
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 4_000_000: raise ValueError('Request too large')
                body = json.loads(self.rfile.read(length)); doc = body['document']; now = int(time.time() * 1000)
                leases = body.get('leases', {}); removals = body.get('remove', []); accepted=[]; skipped=[]
                if not isinstance(doc, str) or not 1 <= len(doc) <= 200 or not isinstance(leases, dict) or not isinstance(removals, list): raise ValueError('Invalid lease update')
                state_changed=False
                with LOCK, DB:
                    state_changed=DB.execute('DELETE FROM leases WHERE expires<=?', (now,)).rowcount>0
                    for entity in removals:
                        prior=DB.execute('SELECT session,actor FROM leases WHERE document=? AND entity=?', (doc,entity)).fetchone()
                        if prior and (not body.get('session') or prior[0]==body.get('session')) and prior[1]==body.get('actor'):
                            DB.execute('INSERT INTO lease_revocations(document,entity,session,actor,released) VALUES(?,?,?,?,?) ON CONFLICT(document,entity,session,actor) DO UPDATE SET released=excluded.released', (doc,entity,prior[0],prior[1],now+1))
                        if body.get('session'):
                            state_changed|=DB.execute('DELETE FROM leases WHERE document=? AND entity=? AND session=? AND actor=?', (doc, entity, body.get('session'), body.get('actor'))).rowcount>0
                        else:
                            state_changed|=DB.execute('DELETE FROM leases WHERE document=? AND entity=? AND actor=?', (doc, entity, body.get('actor'))).rowcount>0
                    for entity, lease in leases.items():
                        if not isinstance(lease, dict) or not isinstance(lease.get('actor'), str) or not isinstance(lease.get('session'), str): raise ValueError('Invalid lease')
                        expires = min(int(lease.get('expires', 0)),now+15_000)
                        existing=DB.execute('SELECT session,actor,expires,payload FROM leases WHERE document=? AND entity=?', (doc,entity)).fetchone()
                        revoked=DB.execute('SELECT released FROM lease_revocations WHERE document=? AND entity=? AND session=? AND actor=?', (doc,entity,lease['session'],lease['actor'])).fetchone()
                        created=int(lease.get('created',0))
                        if expires > now and (not revoked or created>=revoked[0]) and (not existing or existing[2]<=now or (existing[0]==lease['session'] and existing[1]==lease['actor'])):
                            normalized=dict(lease);normalized['expires']=expires;encoded=json.dumps(normalized, sort_keys=True)
                            if not existing or existing[0]!=lease['session'] or existing[1]!=lease['actor'] or existing[2]!=expires or existing[3]!=encoded:
                                DB.execute('INSERT INTO leases(document,entity,session,actor,expires,payload) VALUES(?,?,?,?,?,?) ON CONFLICT(document,entity) DO UPDATE SET session=excluded.session,actor=excluded.actor,expires=excluded.expires,payload=excluded.payload',
                                           (doc, entity, lease['session'], lease['actor'], expires, encoded));state_changed=True
                            accepted.append(entity)
                        elif existing and existing[2]>now:
                            skipped.append({'key':entity,'actor':existing[1],'session':existing[0]})
                    for preview in body.get('previews', {}).values():
                        if isinstance(preview, dict) and isinstance(preview.get('session'),str) and isinstance(preview.get('actor'),str):
                            pkey=(doc,preview['session']);entities=preview.get('entities',[]);limit=preview.get('expires',0);valid=isinstance(entities,list) and bool(entities)
                            if not 0<len(preview['session'])<=200 or not 0<len(preview['actor'])<=200 or type(limit) is not int or not isinstance(entities,list) or not 1<=len(entities)<=200: raise ValueError('Invalid preview')
                            for entity in entities:
                                if not isinstance(entity,dict) or entity.get('kind') not in ('object','stroke') or not isinstance(entity.get('id'),str) or not 0<len(entity['id'])<=200 or type(entity.get('page',0)) is not int or not 0<=entity.get('page',0)<=10000: raise ValueError('Invalid preview entity')
                                ref=f"{int(entity.get('page',0))}:{entity.get('kind')}:{entity.get('id')}";row=DB.execute('SELECT payload FROM leases WHERE document=? AND entity=?',(doc,ref)).fetchone();lease=json.loads(row[0]) if row else {}
                                if lease.get('actor')!=preview['actor'] or lease.get('session')!=preview['session'] or lease.get('expires',0)<=now: valid=False;break
                                limit=min(limit,lease['expires'])
                            old=PREVIEWS.get(pkey)
                            if valid and type(preview.get('expires')) is int and preview.get('expires',0)>now and limit>now:
                                normalized=dict(preview);normalized['expires']=min(preview['expires'],limit,now+15_000)
                                if old!=normalized: PREVIEWS[pkey]=normalized;state_changed=True
                            elif old is not None and old.get('actor')==preview['actor']:
                                PREVIEWS.pop(pkey,None);state_changed=True
                    for session in body.get('clearPreviews', []):
                        old=PREVIEWS.get((doc,session))
                        if old is not None and old.get('actor')==body.get('actor'):
                            PREVIEWS.pop((doc,session),None);state_changed=True
                if state_changed:
                    with CHANGES: GENERATION += 1; CHANGES.notify_all()
                return self.reply(200, {'accepted': accepted, 'selected': accepted, 'skipped': skipped})
            except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
                return self.reply(400, {'error': str(e)})
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
                now = int(time.time() * 1000); DB.execute('DELETE FROM leases WHERE expires<=?', (now,))
                leases = {entity: json.loads(payload) for entity, payload in DB.execute('SELECT entity,payload FROM leases WHERE document=?', (doc,))}
                layers = {}; object_refs={}; layer_orders={}
                for (payload,) in DB.execute('SELECT payload FROM events WHERE document=? ORDER BY seq', (doc,)):
                    prior=json.loads(payload); prior_actions=prior.get('actions',[]) if prior.get('kind')=='batch' else [prior]
                    for action in prior_actions:
                        if action.get('kind') in ('object','stroke'):
                            kind='object' if action.get('kind')=='object' else 'stroke'; layers[(kind,action.get(kind),int(action.get('page',0)))]=action.get('layer')
                            if kind=='object': object_refs[action.get('object')]=action
                        elif action.get('kind')=='layer': layer_orders[action.get('layer')]=action.get('order',layer_orders.get(action.get('layer')))
                for op in ops:
                    actions = op.get('actions', []) if op.get('kind') == 'batch' else [op]
                    actor = op.get('actor', '')
                    session = op.get('session', '')
                    for action in actions:
                        kind = action.get('kind'); entity_kind = 'object' if kind in ('object','object-active') else 'stroke' if kind in ('stroke','active') else None
                        locked_entities=[]
                        if entity_kind:
                            entity_id=action.get(entity_kind)
                            pages=[int(action.get('page',0))] if 'page' in action else [page for (found_kind,found_id,page) in layers if found_kind==entity_kind and found_id==entity_id]
                            if kind in ('object','stroke'):
                                pages.extend(page for (found_kind,found_id,page) in layers if found_kind==entity_kind and found_id==entity_id)
                                pages=list(set(pages))
                            if not pages: pages=[0]
                            locked_entities.extend(f"{page}:{entity_kind}:{entity_id}" for page in pages)
                            if kind in ('object','stroke'):
                                page=int(action.get('page',0)); layers[(entity_kind,entity_id,page)]=action.get('layer')
                                if kind=='object':
                                    for connector_id,connector in object_refs.items():
                                        if connector_id!=entity_id and (connector.get('from')==entity_id or connector.get('to')==entity_id):
                                            locked_entities.append(f"{int(connector.get('page',0))}:object:{connector_id}")
                                    object_refs[entity_id]=action
                        if kind=='layer-active':
                            layer=action.get('layer');locked_entities.extend(f"{page}:{entity_type}:{entity_id}" for (entity_type,entity_id,page),item_layer in layers.items() if item_layer==layer)
                        if kind=='layer' and 'order' in action:
                            layer=action.get('layer')
                            if layer_orders.get(layer)!=action.get('order'):
                                locked_entities.extend(f"{page}:{entity_type}:{entity_id}" for (entity_type,entity_id,page),item_layer in layers.items() if item_layer==layer)
                            layer_orders[layer]=action.get('order')
                        for entity in locked_entities:
                            lease = leases.get(entity)
                            if not lease:
                                entity_kind,entity_id=entity.split(':',2)[1:]
                                lease=next((value for lease_key,value in leases.items() if lease_key.endswith(f":{entity_kind}:{entity_id}")),None)
                            if lease and (lease.get('actor') != actor or lease.get('session') != session): raise ValueError('Locked entity: ' + entity)
                for op in ops:
                    existing = DB.execute('SELECT payload FROM events WHERE document=? AND operation=?', (doc, op['id'])).fetchone()
                    payload = json.dumps(op, sort_keys=True)
                    if existing and existing[0] != payload:
                        raise ValueError('Operation id reused with different contents')
                    DB.execute('INSERT OR IGNORE INTO events(document, operation, payload) VALUES(?,?,?)', (doc, op['id'], payload))
                    if op['kind'] == 'document-delete':
                        DB.execute('INSERT OR IGNORE INTO deleted_documents(document) VALUES(?)', (doc,))
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
    DB.execute('CREATE TABLE IF NOT EXISTS deleted_documents(document TEXT PRIMARY KEY)')
    DB.execute('CREATE TABLE IF NOT EXISTS leases(document TEXT NOT NULL, entity TEXT NOT NULL, session TEXT NOT NULL, actor TEXT NOT NULL, expires INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(document,entity))')
    DB.execute('CREATE TABLE IF NOT EXISTS lease_revocations(document TEXT NOT NULL, entity TEXT NOT NULL, session TEXT NOT NULL, actor TEXT NOT NULL, released INTEGER NOT NULL, PRIMARY KEY(document,entity,session,actor))')
    DB.execute('CREATE TABLE IF NOT EXISTS previews(document TEXT NOT NULL, session TEXT NOT NULL, expires INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(document,session))')
    # Lease and preview coordination is intentionally process-scoped; a server restart releases every selection.
    DB.execute('DELETE FROM leases')
    DB.execute('DELETE FROM previews')
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

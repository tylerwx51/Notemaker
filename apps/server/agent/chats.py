"""Durable chat/event journal, independent of the document operation protocol."""
import json
from pathlib import Path
import sqlite3
import threading
import time
import uuid
from .harness import Cancelled, MockHarness, PiHarness

DEFAULT_MODELS = [
    {'provider': 'openai', 'model': 'gpt-6-luna', 'efforts': ['low', 'medium', 'high']},
    {'provider': 'openai', 'model': 'gpt-5.6-luna', 'efforts': ['low', 'medium', 'high']},
]


class ChatError(ValueError):
    def __init__(self, message, status=400):
        super().__init__(message)
        self.status = status


def string(body, key, maximum=200):
    value = body.get(key)
    if not isinstance(value, str) or not 0 < len(value.strip()) <= maximum:
        raise ChatError('Invalid ' + key)
    return value


class Chats:
    def __init__(self, database, root, data, executable, upstream, models=None):
        self.cv = threading.Condition(threading.RLock())
        self.db = sqlite3.connect(database, check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute('PRAGMA journal_mode=WAL')
        self.db.executescript('''
          CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, title TEXT NOT NULL,
            config TEXT NOT NULL, updated REAL NOT NULL, status TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS chat_events(seq INTEGER PRIMARY KEY AUTOINCREMENT,
            chat TEXT NOT NULL, payload TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS chat_events_chat_seq ON chat_events(chat, seq);
          CREATE TABLE IF NOT EXISTS chat_requests(chat TEXT NOT NULL, request TEXT NOT NULL,
            payload TEXT NOT NULL, UNIQUE(chat, request));
        ''')
        self.models = models if models is not None else DEFAULT_MODELS
        if not isinstance(self.models, list) or not self.models:
            raise ValueError('Chat models must be a nonempty JSON array')
        for model in self.models:
            string(model, 'provider'); string(model, 'model')
            if not isinstance(model.get('efforts'), list) or not model['efforts'] or any(
                effort not in ('off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max') for effort in model['efforts']):
                raise ValueError('Invalid model efforts')
        self.data = Path(data).resolve()
        self.data.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.harnesses = {'mock': MockHarness(), 'pi': PiHarness(root, data, executable, upstream)}
        self.active = {}
        self.closed = False
        # Never repeat an uncertain agent edit after a server crash.
        with self.cv, self.db:
            for row in self.db.execute("SELECT id FROM chats WHERE status='running'").fetchall():
                self._event(row['id'], dict(kind='status', status='interrupted', text='Server restarted. Review activity before sending another message.'))
                self.db.execute("UPDATE chats SET status='interrupted' WHERE id=?", (row['id'],))

    def options(self):
        return {'harnesses': ['pi', 'mock'], 'models': self.models,
                'defaults': {'harness': 'pi', 'provider': self.models[0]['provider'],
                             'model': self.models[0]['model'], 'effort': 'medium' if 'medium' in self.models[0]['efforts'] else self.models[0]['efforts'][0]}}

    def _chat(self, identity):
        row = self.db.execute('SELECT * FROM chats WHERE id=?', (identity,)).fetchone()
        if row is None:
            raise ChatError('Chat not found', 404)
        return dict(row, **json.loads(row['config'])) | {'config': json.loads(row['config'])}

    def get(self, identity):
        with self.cv:
            return self._chat(identity)

    def list(self, limit=5, offset=0):
        if not 1 <= limit <= 100 or offset < 0:
            raise ChatError('Invalid pagination')
        with self.cv:
            ids = self.db.execute('SELECT id FROM chats ORDER BY updated DESC, id DESC LIMIT ? OFFSET ?', (limit + 1, offset)).fetchall()
            return {'chats': [self._chat(row['id']) for row in ids[:limit]], 'more': len(ids) > limit}

    def create(self, body):
        # Client-generated UUID makes retrying creation safe too.
        identity = string(body, 'id')
        try:
            if str(uuid.UUID(identity)) != identity:
                raise ValueError()
        except ValueError:
            raise ChatError('Chat id must be a canonical UUID')
        config = {key: string(body, key) for key in ('harness', 'provider', 'model', 'effort')}
        if config['harness'] not in self.harnesses:
            raise ChatError('Unknown harness')
        if not any(config['provider'] == model['provider'] and config['model'] == model['model'] and config['effort'] in model['efforts'] for model in self.models):
            raise ChatError('Unsupported model or effort')
        with self.cv, self.db:
            existing = self.db.execute('SELECT config FROM chats WHERE id=?', (identity,)).fetchone()
            if existing:
                if json.loads(existing['config']) != config:
                    raise ChatError('Chat id already used', 409)
            else:
                self.db.execute('INSERT INTO chats VALUES(?,?,?,?,?)', (identity, 'New chat', json.dumps(config), time.time(), 'idle'))
            return self._chat(identity)

    def _event(self, identity, event):
        self.db.execute('INSERT INTO chat_events(chat,payload) VALUES(?,?)', (identity, json.dumps(event)))
        self.cv.notify_all()

    def emit(self, identity, event):
        with self.cv, self.db:
            self._event(identity, event)

    def events(self, identity, after=0, wait=0):
        if after < 0:
            raise ChatError('Invalid cursor')
        with self.cv:
            self._chat(identity)
            deadline = time.monotonic() + wait
            while True:
                rows = self.db.execute('SELECT seq,payload FROM chat_events WHERE chat=? AND seq>? ORDER BY seq LIMIT 500', (identity, after)).fetchall()
                if rows or time.monotonic() >= deadline or self.closed:
                    return [dict(seq=row['seq'], event=json.loads(row['payload'])) for row in rows]
                self.cv.wait(max(0, deadline - time.monotonic()))

    def send(self, identity, body, documents):
        prompt = {key: string(body, key, 32000 if key == 'text' else 200) for key in ('request', 'text', 'document')}
        # Older clients and already saved retry payloads may lack a UI session.
        if 'session' in body:
            prompt['session'] = string(body, 'session')
        payload = json.dumps(prompt, sort_keys=True)
        with self.cv, self.db:
            chat = self._chat(identity)
            old = self.db.execute('SELECT payload FROM chat_requests WHERE chat=? AND request=?', (identity, prompt['request'])).fetchone()
            if old:
                if old['payload'] != payload:
                    raise ChatError('Message id reused with different contents', 409)
                return {'accepted': prompt['request']}
            if self.closed:
                raise ChatError('Server shutting down', 503)
            if identity in self.active:
                raise ChatError('This chat is still running', 409)
            if len(self.active) >= 4:
                raise ChatError('Four chats are already running; stop one or try again', 409)
            self.db.execute('INSERT INTO chat_requests VALUES(?,?,?)', (identity, prompt['request'], payload))
            self._event(identity, dict(kind='user', id=prompt['request'], text=prompt['text'], document=prompt['document'], **({'session': prompt['session']} if 'session' in prompt else {})))
            self._event(identity, dict(kind='status', status='running', text='Working…'))
            self.db.execute("UPDATE chats SET status='running', updated=?, title=CASE WHEN title='New chat' THEN ? ELSE title END WHERE id=?", (time.time(), prompt['text'][:70], identity))
            cancel = threading.Event()
            thread = threading.Thread(target=self._run, args=(chat, prompt, cancel, documents), daemon=True)
            self.active[identity] = (cancel, thread)
            # Commit before the worker can emit anything or execute tools.
            self.db.commit()
            thread.start()
            return {'accepted': prompt['request']}

    def _run(self, chat, prompt, cancel, documents):
        identity = chat['id']
        status, detail = 'done', 'Finished'
        try:
            self.harnesses[chat['harness']].run(chat, prompt, lambda event: self.emit(identity, event), cancel, documents=documents)
            if cancel.is_set():
                raise Cancelled()
        except Cancelled:
            status, detail = 'stopped', 'Stopped. Completed edits remain saved.'
        except Exception as error:
            status, detail = 'error', str(error)
        finally:
            with self.cv, self.db:
                self._event(identity, dict(kind='status', status=status, text=detail))
                self.db.execute('UPDATE chats SET status=?, updated=? WHERE id=?', (status, time.time(), identity))
                self.active.pop(identity, None)

    def stop(self, identity):
        with self.cv:
            self._chat(identity)
            if identity in self.active:
                self.active[identity][0].set()
            return {'stopping': identity in self.active}

    def close(self):
        with self.cv:
            self.closed = True
            workers = list(self.active.values())
            for cancel, _ in workers:
                cancel.set()
            self.cv.notify_all()
        for _, thread in workers:
            thread.join(timeout=8)
        self.harnesses['pi'].close()

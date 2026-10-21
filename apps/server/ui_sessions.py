"""Ephemeral live-screen presence and reverse commands for the proxy tree."""

import threading
import time
import uuid


class UiSessions:
    LEASE_MS = 12000
    COMMAND_SECONDS = 32

    def __init__(self):
        self.changed = threading.Condition()
        self.sessions = {}
        self.pending = {}
        self.completed = {}

    @staticmethod
    def _text(value, name):
        if not isinstance(value, str) or not 1 <= len(value) <= 200:
            raise ValueError('Invalid ' + name)
        return value

    def _expire(self, now):
        for session, record in list(self.sessions.items()):
            if record['expires'] <= now:
                del self.sessions[session]
                for request in self.pending.values():
                    if request['session'] == session and request['result'] is None:
                        request['result'] = {'error': 'App session disconnected'}
                self.changed.notify_all()
        for identity, expired in list(self.completed.items()):
            if expired <= now:
                del self.completed[identity]

    def list(self):
        with self.changed:
            self._expire(int(time.time() * 1000))
            return {'sessions': sorted(self.sessions.values(), key=lambda row: row['session']),
                    'scope': 'root'}

    def heartbeat(self, body):
        proxy = self._text(body.get('proxyId'), 'proxy ID')
        incoming = body.get('sessions')
        results = body.get('results', [])
        if not isinstance(incoming, list) or len(incoming) > 32 or not isinstance(results, list) or len(results) > 64:
            raise ValueError('Invalid UI heartbeat')
        now = int(time.time() * 1000)
        with self.changed:
            self._expire(now)
            reported = set()
            for state in incoming:
                if not isinstance(state, dict):
                    raise ValueError('Invalid UI session state')
                session = self._text(state.get('session'), 'session ID')
                reported.add(session)
                previous = self.sessions.get(session)
                if previous and previous['proxyId'] != proxy and previous['expires'] > now:
                    raise ValueError('Session ID belongs to another proxy')
                self.sessions[session] = dict(state, proxyId=proxy, lastSeen=now, expires=now + self.LEASE_MS)
            for session, record in list(self.sessions.items()):
                if record['proxyId'] == proxy and session not in reported:
                    del self.sessions[session]
                    for request in self.pending.values():
                        if request['session'] == session and request['result'] is None:
                            request['result'] = {'error': 'App session disconnected'}
                    self.changed.notify_all()
            acknowledged = []
            for result in results:
                if not isinstance(result, dict):
                    raise ValueError('Invalid UI result')
                identity = self._text(result.get('requestId'), 'request ID')
                request = self.pending.get(identity)
                if request and request['proxyId'] == proxy:
                    request['result'] = {'error': result['error']} if 'error' in result else {'value': result.get('value')}
                    self.completed[identity] = now + 60000
                    self.changed.notify_all()
                    acknowledged.append(identity)
                elif identity in self.completed:
                    acknowledged.append(identity)
            commands = [dict(requestId=identity, session=request['session'], verb=request['verb'],
                             action=request['action'], input=request['input'])
                        for identity, request in self.pending.items()
                        if request['proxyId'] == proxy and request['result'] is None]
            return {'commands': commands, 'acknowledged': acknowledged, 'serverTime': now}

    def command(self, body):
        session = self._text(body.get('session'), 'session ID')
        verb = self._text(body.get('verb'), 'UI verb')
        action = body.get('action', '')
        if verb not in ('state', 'actions', 'action') or not isinstance(action, str) or len(action) > 200:
            raise ValueError('Invalid UI command')
        if verb == 'action' and not action:
            raise ValueError('UI action name is required')
        data = body.get('input', {})
        if not isinstance(data, dict):
            raise ValueError('UI action input must be an object')
        now = int(time.time() * 1000)
        with self.changed:
            self._expire(now)
            owner = self.sessions.get(session)
            if owner is None:
                raise ValueError('App session is not connected: ' + session)
            identity = str(uuid.uuid4())
            request = dict(session=session, proxyId=owner['proxyId'], verb=verb,
                           action=action, input=data, result=None)
            self.pending[identity] = request
            deadline = time.monotonic() + self.COMMAND_SECONDS
            try:
                while request['result'] is None:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise ValueError('UI command timed out; inspect session state before retrying')
                    self.changed.wait(min(remaining, 1))
                    self._expire(int(time.time() * 1000))
                if 'error' in request['result']:
                    raise ValueError(str(request['result']['error']))
                return {'result': request['result']['value']}
            finally:
                self.pending.pop(identity, None)
                self.completed[identity] = int(time.time() * 1000) + 60000

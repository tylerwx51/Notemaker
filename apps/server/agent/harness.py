"""Harness adapters. Pi's executable is supplied by the host, never downloaded here."""
import collections
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import threading
import time
import uuid
import urllib.request


class Cancelled(Exception):
    pass


class MockHarness:
    def run(self, chat, prompt, emit, cancel, **_):
        emit(dict(kind='tool', id='mock-' + prompt['request'], name='mock.inspect',
                  args=json.dumps({'document': prompt['document']}), status='running', text=''))
        if cancel.wait(.2):
            raise Cancelled()
        emit(dict(kind='tool_end', id='mock-' + prompt['request'], status='done',
                  text='Testing mock: no document was read or changed.'))
        message = str(uuid.uuid4())
        emit(dict(kind='assistant', id=message, text=''))
        answer = 'Mock response: ' + prompt['text'] + '\nThis testing harness makes no edits.'
        for offset in range(0, len(answer), 8):
            if cancel.wait(.06):
                raise Cancelled()
            emit(dict(kind='delta', id=message, text=answer[offset:offset + 8]))
        emit(dict(kind='assistant_end', id=message, text=answer))


class PiHarness:
    def __init__(self, root, data, executable, upstream):
        self.root, self.data = Path(root).resolve(), Path(data).resolve()
        self.executable, self.upstream = executable, upstream
        self.proxy = None
        self.proxy_lock = threading.Lock()
        self.config = self.data / 'pi-home'
        self.config.mkdir(parents=True, exist_ok=True, mode=0o700)
        # A dedicated writable credential copy permits OAuth refresh without exposing
        # the user's other Pi sessions or home directory to the agent.
        source = Path(os.environ.get('PI_CODING_AGENT_DIR', Path.home() / '.pi/agent'))
        for name in ('auth.json', 'models.json'):
            original, destination = source / name, self.config / name
            if original.exists() and (not destination.exists() or original.stat().st_mtime > destination.stat().st_mtime):
                shutil.copyfile(original, destination)
                destination.chmod(0o600)

    def ensure_proxy(self):
        with self.proxy_lock:
            if self.proxy is not None and self.proxy.poll() is None:
                return
            replica = self.data / 'replica'
            # A proxy can outlive a killed main-server process. Reuse that owner
            # instead of starting a second writer or abandoning its durable queue.
            try:
                descriptor=json.loads((replica / 'proxy.json').read_text())
                request=urllib.request.Request(descriptor['url']+'/api/info',headers={'Authorization':'Bearer '+descriptor['token']})
                with urllib.request.urlopen(request,timeout=2) as response:
                    peer=json.load(response)
                if peer.get('kind')=='proxy' and peer.get('id')==descriptor.get('id'):
                    return
            except (OSError, ValueError, KeyError):
                pass
            self.proxy = subprocess.Popen([str(self.root / 'apps/cli/notemaker'), '--state', str(replica),
                                           'serve', '0', self.upstream], stdout=subprocess.PIPE,
                                          stderr=subprocess.DEVNULL, text=True)
            ready = self.proxy.stdout.readline()
            self.proxy.stdout.close()
            if not ready:
                self.proxy.wait(timeout=5)
                raise ValueError('Cannot start the agent replica proxy; build the CLI first')

    def close(self):
        with self.proxy_lock:
            if self.proxy is not None and self.proxy.poll() is None:
                self.proxy.terminate()
                self.proxy.wait(timeout=10)

    def command(self, chat):
        pi = shutil.which(self.executable)
        if not pi:
            raise ValueError('Pi executable unavailable; set --pi-command')
        node, bwrap = shutil.which('node'), shutil.which('bwrap')
        if not node or not bwrap:
            raise ValueError('Run the server inside nix-shell (node and bubblewrap required)')
        self.ensure_proxy()
        pi = Path(pi).resolve()
        package = pi.parent
        while package != package.parent and not (package / 'package.json').exists():
            package = package.parent
        if not (package / 'package.json').exists():
            raise ValueError('Pi command must point to the installed JavaScript CLI')
        args = [bwrap, '--die-with-parent', '--new-session', '--unshare-all', '--share-net',
                '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp']
        # Runtime and DNS/certificate inputs only; no bind of / or the user's home.
        for path in ('/nix/store', '/bin', '/usr/bin', '/etc/ssl', '/etc/resolv.conf', '/etc/hosts', '/etc/localtime'):
            if Path(path).exists():
                args += ['--ro-bind', path, path]
        args += ['--ro-bind', str(package), str(package),
                 '--bind', str(self.root), str(self.root)]
        if not self.data.is_relative_to(self.root):
            args += ['--bind', str(self.data), str(self.data)]
        args += ['--chdir', str(self.root), '--clearenv']
        env = {'PATH': os.environ['PATH'], 'JAVA_HOME': os.environ.get('JAVA_HOME', ''),
               'HOME': '/tmp/home', 'TMPDIR': '/tmp', 'LANG': 'C.UTF-8',
               'IN_NIX_SHELL': 'impure', 'PI_CODING_AGENT_DIR': str(self.config),
               'PI_OFFLINE': '1', 'PI_TELEMETRY': '0',
               'NOTEMAKER_EVENT_FILE': str(self.data / (chat['id'] + '.events.jsonl')),
               'NOTEMAKER_STATE': str(self.data / 'replica'),
               'NOTEMAKER_PROXY_FILE': str(self.data / 'replica' / 'proxy.json'),
               'NOTEMAKER_ACTOR': 'agent-' + chat['id'],
               'NODE_EXTRA_CA_CERTS': str(Path('/etc/ssl/certs/ca-certificates.crt').resolve())}
        for key in ('SSL_CERT_FILE', 'NIX_SSL_CERT_FILE', 'NOTEMAKER_JSVG'):
            if key in os.environ:
                env[key] = os.environ[key]
        for key, value in env.items():
            args += ['--setenv', key, value]
        instructions = (
            'You are the NoteMaker tablet assistant. Work in this repository. Use the '
            'notemaker-server skill and CLI documentation to read and edit notes without '
            'asking permission for edits. All document mutations must use the CLI local '
            'proxy; never modify replica files or server tables. NOTEMAKER_ACTOR and '
            'NOTEMAKER_PROXY_FILE are already set to your stable actor and selected proxy. '
            'Use these CLI defaults. If explicit flags are needed, use '
            '--actor "$NOTEMAKER_ACTOR" --proxy-file "$NOTEMAKER_PROXY_FILE"; never invent or '
            'retype the actor ID or replica path. '
            'The upstream is ' + self.upstream + '. Synchronize before reading and after '
            'editing each document. The host already launched you inside nix-shell, and '
            'all required CLI runtime tools are on PATH; no nested nix-shell is needed. '
            'The filesystem is sandboxed; the network guide outside this checkout is '
            'intentionally unavailable. Use this upstream instead. Discover needed context '
            'using the CLI. Preserve whole diagrams as atomic actions and use your actor '
            'for undo. Do not launch other agents. Explain what you changed in your reply.'
        )
        return args + [node, str(pi), '--mode', 'rpc', '--session', str(self.data / (chat['id'] + '.jsonl')),
                       '--provider', chat['provider'], '--model', chat['model'], '--thinking', chat['effort'],
                       '--no-extensions', '--no-mcp', '--no-prompt-templates', '--no-themes', '--approve',
                       '--extension', str(self.root / 'apps/server/agent/stream.ts'),
                       '--skill', str(self.root / '.agents/skills/notemaker-server/SKILL.md'),
                       '--append-system-prompt', instructions]

    def run(self, chat, prompt, emit, cancel, documents, **_):
        command = self.command(chat)
        event_path = self.data / (chat['id'] + '.events.jsonl')
        event_path.touch(mode=0o600)
        event_path.write_bytes(b'')
        process = subprocess.Popen(command, cwd=self.root, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        errors = collections.deque(maxlen=12)
        def drain():
            for line in process.stderr:
                errors.append(line.decode('utf-8', errors='replace')[:2000])
        reader = threading.Thread(target=drain, daemon=True)
        reader.start()
        done = threading.Event()
        timed_out = threading.Event()
        def watchdog():
            deadline = time.monotonic() + 1800
            while not done.wait(.1):
                if cancel.is_set() or time.monotonic() > deadline:
                    if not cancel.is_set():
                        timed_out.set()
                    # Entire sandbox namespace dies with its init; kill the host group too.
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        return
                    if not done.wait(2):
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    return
        watcher = threading.Thread(target=watchdog, daemon=True)
        watcher.start()
        observed = {'ready': False, 'failed': False, 'error': None}
        def read_hooks():
            try:
                with event_path.open('rb') as source:
                    pending = b''
                    while True:
                        chunk = source.readline()
                        if chunk:
                            pending += chunk
                            if not pending.endswith(b'\n'):
                                continue
                            event = json.loads(pending)
                            pending = b''
                            if event['kind'] == 'ready':
                                observed['ready'] = True
                            else:
                                if event['kind'] == 'assistant':
                                    observed['failed'] = False
                                observed['failed'] = observed['failed'] or event['kind'] == 'error'
                                emit(event)
                        elif done.is_set():
                            break
                        else:
                            done.wait(.025)
            except Exception as error:
                observed['error'] = error
                cancel.set()
        hook_reader = threading.Thread(target=read_hooks, daemon=True)
        hook_reader.start()
        settled = False
        try:
            # Small routing context only, not a prebuilt dump of the user's notes.
            message = json.dumps({'current_document': prompt['document'], 'server_documents': documents}, ensure_ascii=False)
            message += '\n\n' + prompt['text']
            process.stdin.write((json.dumps(dict(id=prompt['request'], type='prompt', message=message)) + '\n').encode())
            process.stdin.flush()
            for line in process.stdout:
                record = json.loads(line)
                if record.get('type') == 'response' and not record.get('success', True):
                    raise RuntimeError(record.get('error', 'Pi rejected prompt'))
                elif record.get('type') == 'extension_error':
                    raise RuntimeError('Pi streaming hook failed: ' + record.get('error', 'unknown error'))
                elif record.get('type') == 'agent_settled':
                    settled = True
                    if record.get('aborted'):
                        raise Cancelled()
                    break
            done.set()
            hook_reader.join(timeout=10)
            if observed['error']:
                raise RuntimeError('Cannot read Pi hooks: ' + str(observed['error']))
            if cancel.is_set():
                raise Cancelled()
            if timed_out.is_set():
                raise RuntimeError('Agent exceeded the 30 minute run limit')
            if not observed['ready'] or not settled:
                raise RuntimeError('Pi exited before completion. ' + ''.join(errors)[-2000:])
            if observed['failed']:
                raise RuntimeError('Pi reported a provider error; see preceding event')
        finally:
            done.set()
            process.stdin.close()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=3)
            process.stdout.close()
            reader.join(timeout=2)
            process.stderr.close()
            watcher.join(timeout=2)
            hook_reader.join(timeout=10)
            event_path.unlink(missing_ok=True)

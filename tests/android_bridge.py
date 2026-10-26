"""Persistent, loopback-only command channel for Android instrumentation checks."""
import json
from pathlib import Path
import socket
import subprocess
import time


class AndroidBridge:
    def __init__(self, serial, out):
        self.serial = serial
        self.out = Path(out)
        self.process = None
        self.socket = None
        self.stream = None
        self.forward = None
        self.log = None
        self.launches = 0

    def adb(self, *args, timeout=20):
        return subprocess.check_output(['adb', '-s', self.serial, *args], text=True,
                                       stderr=subprocess.STDOUT, timeout=timeout)

    def start(self):
        if self.process is not None and self.process.poll() is None:
            return
        self.close()
        self.adb('shell', 'run-as', 'com.notemaker', 'rm', '-f', 'files/integration-bridge-port')
        self.launches += 1
        self.log = (self.out / f'bridge-{self.serial}-{self.launches}.log').open('w')
        self.process = subprocess.Popen(['adb', '-s', self.serial, 'shell', 'am', 'instrument',
                                         '-w', '-e', 'action', 'bridge', 'com.notemaker/.Integration'],
                                        stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f'Android instrumentation bridge exited; see {self.log.name}')
            try:
                port = int(self.adb('shell', 'run-as', 'com.notemaker', 'cat',
                                    'files/integration-bridge-port', timeout=3).strip())
                self.forward = int(self.adb('forward', 'tcp:0', f'tcp:{port}').strip())
                self.socket = socket.create_connection(('127.0.0.1', self.forward), timeout=15)
                self.socket.settimeout(120)
                self.stream = self.socket.makefile('rwb')
                return
            except (OSError, ValueError, subprocess.CalledProcessError):
                time.sleep(.1)
        raise TimeoutError(f'Android instrumentation bridge did not start; see {self.log.name}')

    def command(self, action, **args):
        self.start()
        value = dict(action=action, **{key: str(item) for key, item in args.items()})
        self.stream.write((json.dumps(value) + '\n').encode())
        self.stream.flush()
        line = self.stream.readline()
        if not line:
            raise RuntimeError(f'Android instrumentation bridge closed during {action}; see {self.log.name}')
        result = json.loads(line)
        assert f'PASS {action}' in result.get('stream', ''), result.get('stream', result)
        return result

    def ui_nodes(self):
        return json.loads(self.command('ui-tree')['nodes'])

    def close(self):
        if self.stream is not None:
            try:
                self.stream.write(b'{"action":"shutdown"}\n')
                self.stream.flush()
                self.stream.readline()
            except (OSError, ValueError):
                pass
            self.stream.close()
            self.stream = None
        if self.socket is not None:
            self.socket.close()
            self.socket = None
        if self.forward is not None:
            try:
                self.adb('forward', '--remove', f'tcp:{self.forward}')
            except (OSError, subprocess.CalledProcessError):
                pass
            self.forward = None
        if self.process is not None:
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
            self.process = None
        if self.log is not None:
            self.log.close()
            self.log = None

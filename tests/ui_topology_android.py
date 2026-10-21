#!/usr/bin/env python3
"""Live Android session discovery and control through two proxies and a root server."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request
from topology_support import Proxy

ROOT=Path(__file__).resolve().parents[1]
SERIAL='emulator-5594'

def run(*args,timeout=45):
    return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=timeout).strip()

def adb(*args): return run('adb','-s',SERIAL,*args)

def wait(fn,seconds=90):
    deadline=time.monotonic()+seconds;last=None
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,AssertionError,subprocess.CalledProcessError) as error:last=error
        time.sleep(.5)
    raise AssertionError('Timed out waiting for UI topology: '+str(last))

def available_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        return sock.getsockname()[1]

def main():
    assert SERIAL not in run('adb','devices'),'Reserved emulator already running'
    with tempfile.TemporaryDirectory(prefix='notemaker-ui-topology-') as directory:
        root=Path(directory);port=available_port();url=f'http://127.0.0.1:{port}'
        server=subprocess.Popen(['python3',str(ROOT/'apps/server/server.py'),'--host','127.0.0.1','--port',str(port),'--database',str(root/'server.sqlite3')],stdout=subprocess.DEVNULL,stderr=(root/'server.log').open('w'))
        parent=child=emulator=None
        try:
            wait(lambda:json.load(urllib.request.urlopen(url+'/api/info',timeout=2)))
            parent=Proxy(root/'parent',upstream=url)
            child=Proxy(root/'child',upstream=parent.descriptor_path)
            emulator=subprocess.Popen(['boox-emulator-headless','-port','5594','-memory','1536','-cores','2','-wipe-data'],env=dict(os.environ,ANDROID_AVD_HOME=str(root/'avd')),stdout=(root/'emulator.log').open('w'),stderr=subprocess.STDOUT)
            wait(lambda:adb('shell','getprop','sys.boot_completed')=='1',240)
            adb('install','-r',str(ROOT/'apps/android/build/notemaker.apk'))
            adb('shell','am','start','-W','-n','com.notemaker/.MainActivity')
            instrument=adb('shell','am','instrument','-w','-e','action','configure','-e','document','topology-ui','-e','host',f'http://10.0.2.2:{port}','com.notemaker/.Integration')
            assert 'PASS configure' in instrument,instrument
            adb('shell','am','force-stop','com.notemaker')
            adb('shell','am','start','-W','-n','com.notemaker/.MainActivity')
            sessions=wait(lambda:child.cli('ui','sessions')['sessions'])
            record=sessions[0];session=record['session']
            assert record['deviceId'] and record['deviceName'] and record['proxyId']
            assert record['document']=='topology-ui' and record['lastSeen']>0,record
            root_sessions=json.load(urllib.request.urlopen(url+'/api/ui/sessions'))['sessions']
            assert any(row['session']==session for row in root_sessions)
            assert child.cli('ui','state',session)['session']==session
            opened=child.cli('ui','action',session,'panel.open','-',value={'name':'document'})['state']
            assert opened['panelOpen']
            changed=child.cli('ui','action',session,'document.open','-',value={'name':'abc'})['state']
            assert changed['document']=='abc',changed
            assert child.cli('ui','state',session)['document']=='abc'
            adb('shell','am','force-stop','com.notemaker')
            wait(lambda:not child.cli('ui','sessions')['sessions'],25)
            print('PASS root session list, device metadata, two-proxy CLI control, document switch, and expiry')
        finally:
            if emulator is not None:
                try:adb('emu','kill')
                except Exception:pass
                emulator.terminate()
                try:emulator.wait(timeout=15)
                except subprocess.TimeoutExpired:emulator.kill();emulator.wait()
            if child is not None:child.stop()
            if parent is not None:parent.stop()
            server.terminate()
            try:server.wait(timeout=10)
            except subprocess.TimeoutExpired:server.kill();server.wait()

if __name__=='__main__':main()

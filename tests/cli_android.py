#!/usr/bin/env python3
"""App-spawned CLI, offline shared state and three-way synchronization on API 36."""
import base64
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request
import re
import xml.etree.ElementTree as ET
import hashlib
import uuid
from browser import check_browser
from replica import read_replica
from topology_support import Proxy

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.test-artifacts/cli-android'
OUT.mkdir(parents=True,exist_ok=True)
SERIAL='emulator-5584'
PORT=18766
CLI=ROOT/'apps/cli/notemaker'
EXAMPLE=json.loads((ROOT/'apps/cli/examples/diagram.json').read_text())

def command(*args,timeout=90):
    return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=timeout)

def adb(*args):return command('adb','-s',SERIAL,*args)

def wait(fn,seconds=90):
    deadline=time.monotonic()+seconds;last=None
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except Exception as e:last=e
        time.sleep(1)
    raise AssertionError(f'Timed out: {last}')

def instrument(action,**kwargs):
    args=['shell','am','instrument','-w','-e','action',action]
    for key,value in kwargs.items():args+=['-e',key,str(value)]
    out=adb(*args,'com.notemaker/.Integration')
    (OUT/f'{action}.txt').write_text(out)
    assert f'PASS {action}' in out,out
    state=json.loads(adb('shell','run-as','com.notemaker','cat','files/integration.json'))
    (OUT/'latest-state.json').write_text(json.dumps(state,indent=2))
    adb('shell','am','start','-f','0x20000000','-n','com.notemaker/.MainActivity')
    return state

def encoded(value):return base64.b64encode(json.dumps(value).encode()).decode()

def replica():
    return read_replica(SERIAL, "integration")

def object_snapshot(object_id):
    saved=replica();result={}
    def apply(op):
        nonlocal result
        if op['kind']=='batch':
            for action in op['actions']:apply(action)
        elif op['kind']=='object' and op['object']==object_id:result=op
    for op in saved['events']+saved['pending']:apply(op)
    return result

def tap(value):
    adb('shell','uiautomator','dump','/data/local/tmp/cli-ui.xml')
    tree=ET.fromstring(adb('shell','cat','/data/local/tmp/cli-ui.xml'))
    node=next((n for n in tree.iter('node') if n.attrib.get('content-desc')==value or n.attrib.get('text','').casefold()==value.casefold()),None)
    assert node is not None,f'Missing UI control: {value}'
    left,top,right,bottom=map(int,re.findall(r'\d+',node.attrib['bounds']))
    adb('shell','input','tap',str((left+right)//2),str((top+bottom)//2));time.sleep(.3)

def tablet(*args,value=None,concurrent=False):
    kwargs={'cli':encoded(['--actor','tablet-agent',*args])}
    if value is not None:kwargs['input']=encoded(value)
    return instrument('cli-concurrent' if concurrent else 'cli',**kwargs)

def desktop(*args,value=None):
    p=subprocess.run([str(CLI),'--proxy-file',str(desktop_proxy.descriptor_path),'--document','integration','--actor','desktop-agent',*args],input=json.dumps(value) if value is not None else None,text=True,capture_output=True,timeout=30)
    assert p.returncode==0,p.stderr
    return json.loads(p.stdout)

server=None;emulator=None;desktop_proxy=None
try:
    assert SERIAL not in command('adb','devices'),'Reserved emulator already running'
    with socket.socket() as s:s.bind(('127.0.0.1',PORT))
    # Unique directories keep prior artifacts and any personal state untouched.
    with tempfile.TemporaryDirectory(prefix='notemaker-cli-android-') as temp:
        env=dict(os.environ,ANDROID_AVD_HOME=temp+'/avd')
        emulator=subprocess.Popen(['boox-emulator-headless','-port','5584','-memory','1536','-cores','2','-wipe-data'],env=env,stdout=(OUT/'emulator.log').open('w'),stderr=subprocess.STDOUT)
        wait(lambda:adb('shell','getprop','sys.boot_completed').strip()=='1',240)
        adb('install','-r',str(ROOT/'apps/android/build/notemaker.apk'))
        instrument('configure',document='integration',host=f'http://10.0.2.2:{PORT}')
        state=tablet('diagram','add','-',value=EXAMPLE,concurrent=True)
        assert len(state['model']['objects'])==8 and state['strokes']==1 and state['pending']>0,state
        print('PASS app-spawned CLI and pen input share local state offline',flush=True)
        tablet('object','update','app','-',value={'x':125,'text':'Offline agent edit'})
        state=instrument('state')
        assert state['model']['objects'][0]['text']=='Offline agent edit'
        tablet('undo');state=tablet('undo')
        assert state['model']['layerActive']['architecture'] is False
        state=tablet('redo')
        assert state['model']['layerActive']['architecture'] is True
        tablet('redo')
        print('PASS actor-specific atomic diagram undo/redo offline',flush=True)
        svg_source=ROOT/'apps/cli/examples/placed.svg'
        adb('push',str(svg_source),'/data/local/tmp/placed.svg')
        placement=OUT/'svg-placement.json';placement.write_text(json.dumps({'object':'tablet-svg','x':100,'y':1100,'w':400,'h':200}))
        adb('push',str(placement),'/data/local/tmp/svg-placement.json')
        state=tablet('svg','add','/data/local/tmp/placed.svg','/data/local/tmp/svg-placement.json')
        assert state['model']['objects'][-1]['svg']==svg_source.read_text()
        state=tablet('undo');assert state['model']['objectActive']['tablet-svg'] is False
        state=tablet('redo');assert state['model']['objectActive']['tablet-svg'] is True
        print('PASS Android SVG file placement and atomic undo/redo offline',flush=True)
        tablet('export','/data/user/0/com.notemaker/files/agent-export.png')
        data=subprocess.check_output(['adb','-s',SERIAL,'exec-out','run-as','com.notemaker','cat','files/agent-export.png'])
        assert data.startswith(b'\x89PNG')
        (OUT/'android-cli.png').write_bytes(data)
        tap('Document actions');tap('Edit diagram objects…');tap('rect · Offline agent edit')
        (OUT/'object-editor.png').write_bytes(subprocess.check_output(['adb','-s',SERIAL,'exec-out','screencap','-p']))
        tap('Erase')
        state=tablet('inspect');assert state['model']['objectActive']['app'] is False
        tap('Undo and redo');tap('Undo')
        state=tablet('inspect');assert state['model']['objectActive']['app'] is True
        print('PASS object editor erasure and human undo visible to CLI',flush=True)
        # Reset only the reserved desktop test replica by assigning a fresh state directory.
        desktop_proxy=Proxy(Path(temp)/'desktop','integration')
        server=subprocess.Popen(['python3',str(ROOT/'apps/server/server.py'),'--host','127.0.0.1','--port',str(PORT),'--database',temp+'/server.sqlite3'],stdout=(OUT/'server.log').open('w'),stderr=subprocess.STDOUT)
        wait(lambda:urllib.request.urlopen(f'http://127.0.0.1:{PORT}/api/info',timeout=2).status==200)
        wait(lambda:not replica()['pending'])
        desktop('sync',f'http://127.0.0.1:{PORT}')
        assert len(desktop('inspect')['objects'])==9
        desktop('object','update','cli','-',value={'text':'Desktop agent','x':580})
        desktop('layer','put','-',value={'layer':'test-layer','name':'Forced layer','force':True})
        desktop('sync',f'http://127.0.0.1:{PORT}')
        wait(lambda:object_snapshot('cli').get('text')=='Desktop agent')
        print('PASS desktop CLI ↔ server ↔ Android local state convergence',flush=True)
        server.terminate();server.wait(timeout=10);server=None
        tablet('object','hide','app')
        desktop('object','update','app','-',value={'x':250})
        server=subprocess.Popen(['python3',str(ROOT/'apps/server/server.py'),'--host','127.0.0.1','--port',str(PORT),'--database',temp+'/server.sqlite3'],stdout=(OUT/'server.log').open('a'),stderr=subprocess.STDOUT)
        wait(lambda:urllib.request.urlopen(f'http://127.0.0.1:{PORT}/api/info',timeout=2).status==200)
        wait(lambda:not replica()['pending'])
        desktop('sync',f'http://127.0.0.1:{PORT}')
        wait(lambda:object_snapshot('app').get('x')==250)
        state=instrument('state')
        desktop('sync',f'http://127.0.0.1:{PORT}')
        assert state['model']==desktop('inspect')
        assert state['model']['objectActive']['app'] is False
        print('PASS restart/reconnect merges move and erase without resurrection',flush=True)
        desktop('export',str(OUT/'desktop-svg.png'))
        desktop('export',str(OUT/'desktop-svg.svg'))
        stats=check_browser(OUT,PORT,state['cursor'],debug_port=19224,svg_exports=True)
        assert stats['pixels']>5000,stats
        print('PASS browser renders synchronized diagram objects',flush=True)
        adb('shell','am','force-stop','com.notemaker')
        state=instrument('state');assert state['model']==desktop('inspect')
        print('PASS replica and pending/history survive app restart',flush=True)
        # Select another durable proxy as the editor's writer and read source.
        adb('reverse',f'tcp:{desktop_proxy.port}',f'tcp:{desktop_proxy.port}')
        state=instrument('configure',document='integration',host=f'http://10.0.2.2:{PORT}',
                         proxyUrl=f'http://127.0.0.1:{desktop_proxy.port}',proxyToken=desktop_proxy.descriptor['token'])
        assert state['model']==desktop('inspect')
        before=len(desktop('inspect')['strokes'])
        instrument('draw',shift='150')
        wait(lambda:len(desktop('inspect')['strokes'])==before+1)
        tablet('object','update','cli','-',value={'text':'Selected proxy'})
        assert next(o for o in desktop('inspect')['objects'] if o['object']=='cli')['text']=='Selected proxy'
        desktop('object','update','cli','-',value={'text':'SSE update'})
        wait(lambda:next(o for o in instrument('state')['model']['objects'] if o['object']=='cli')['text']=='SSE update')
        print('PASS app and spawned CLI use selected proxy; SSE refreshes remote editor',flush=True)
        state=instrument('configure',document='remote-pages',type='pages',proxyUrl=f'http://127.0.0.1:{desktop_proxy.port}',proxyToken=desktop_proxy.descriptor['token'])
        assert state['type']=='pages',state
        instrument('page',page=1)
        state=instrument('draw')
        assert state['model']['strokes'][0]['page']==1,state
        print('PASS selected proxy creates and renders a multipage document',flush=True)
        adb('emu','kill');emulator.wait(timeout=20);emulator=None
finally:
    if desktop_proxy:desktop_proxy.stop()
    if server:server.terminate();server.wait(timeout=10)
    if emulator:
        try:(OUT/'failure-logcat.txt').write_text(adb('logcat','-d','-s','NoteMaker','AndroidRuntime'))
        except Exception:pass
        try:adb('emu','kill')
        except Exception:emulator.terminate()
        try:emulator.wait(timeout=20)
        except subprocess.TimeoutExpired:emulator.kill()

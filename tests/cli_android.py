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
import shlex
import uuid
from browser import check_browser
from replica import cli_command, read_replica
import threading
from topology_support import Proxy
from android_bridge import AndroidBridge

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.test-artifacts/cli-android'
OUT.mkdir(parents=True,exist_ok=True)
SERIAL='emulator-5584'
PORT=18766
CLI=ROOT/'apps/cli/notemaker'
EXAMPLE=json.loads((ROOT/'apps/cli/examples/diagram.json').read_text())
bridge=AndroidBridge(SERIAL,OUT)

def command(*args,timeout=90):
    return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=timeout)

def adb(*args):
    if args[:3] == ('shell','uiautomator','dump'):
        # UI Automator can exit successfully after failing to obtain an idle
        # hierarchy. Require a fresh, valid dump before callers consume it.
        for attempt in range(5):
            command('adb','-s',SERIAL,'shell','rm','-f',args[3])
            output=command('adb','-s',SERIAL,*args)
            try:
                ET.fromstring(command('adb','-s',SERIAL,'shell','cat',args[3]))
                return output
            except (subprocess.CalledProcessError, ET.ParseError):
                if attempt == 4:raise
                time.sleep(.5)
    return command('adb','-s',SERIAL,*args)

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
    out=bridge.command(action,**kwargs)['stream']
    (OUT/f'{action}.txt').write_text(out)
    assert f'PASS {action}' in out,out
    state=json.loads(adb('shell','run-as','com.notemaker','cat','files/integration.json'))
    (OUT/'latest-state.json').write_text(json.dumps(state,indent=2))
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
    # Tap only once the control is in the same place in two consecutive UI
    # dumps, so a dialog or popup still opening cannot move it from under us.
    node=None;previous=None
    for _ in range(30):
        nodes=bridge.ui_nodes()
        node=next((n for n in nodes if n.get('content-desc')==value or n.get('text','').casefold()==value.casefold()),None)
        if node is not None and previous is not None and node['bounds']==previous['bounds']:break
        previous=node;node=None
        time.sleep(.2)
    if node is None:
        (OUT/'missing-ui.json').write_text(json.dumps(nodes,indent=2))
        (OUT/'missing-ui.png').write_bytes(subprocess.check_output(['adb','-s',SERIAL,'exec-out','screencap','-p']))
        raise AssertionError(f'Missing UI control after waiting for the editor: {value}')
    left,top,right,bottom=map(int,re.findall(r'\d+',node['bounds']))
    adb('shell','input','tap',str((left+right)//2),str((top+bottom)//2));time.sleep(.3)

def canvas_gesture():
    node=wait(lambda: next((n for n in bridge.ui_nodes() if n.get('content-desc')=='Drawing canvas'),None),15)
    left,top,right,bottom=map(int,re.findall(r'\d+',node['bounds']))
    # Left half avoids the layer tray overlay; coordinates follow actual layout.
    x1=left+(right-left)//6;y1=top+(bottom-top)//4
    x2=left+(right-left)//3;y2=top+(bottom-top)//3
    adb('shell','input','swipe',str(x1),str(y1),str(x2),str(y2),'350')

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
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
        s.bind(('127.0.0.1',PORT))
    # Unique directories keep prior artifacts and any personal state untouched.
    with tempfile.TemporaryDirectory(prefix='notemaker-cli-android-') as temp:
        env=dict(os.environ,ANDROID_AVD_HOME=temp+'/avd')
        emulator=subprocess.Popen(['boox-emulator-headless','-port','5584','-memory','1536','-cores','2','-gpu',os.environ.get('NOTEMAKER_TEST_GPU','host'),'-feature','-Vulkan','-wipe-data'],env=env,stdout=(OUT/'emulator.log').open('w'),stderr=subprocess.STDOUT)
        wait(lambda:adb('shell','getprop','sys.boot_completed').strip()=='1',240)
        for setting in ('window_animation_scale','transition_animation_scale','animator_duration_scale'):
            adb('shell','settings','put','global',setting,'0')  # taps must not land on animating dialogs
        adb('install','-r',str(ROOT/'apps/android/build/notemaker.apk'))
        instrument('configure',document='drawing-ui',host='')
        tap('Shape tools')
        wait(lambda:any('Create Triangle' in node.get('content-desc','') for node in bridge.ui_nodes()),15)
        (OUT/'shape-palette.png').write_bytes(subprocess.check_output(['adb','-s',SERIAL,'exec-out','screencap','-p']))
        tap('Create Triangle');canvas_gesture()
        state=instrument('state')
        assert len(state['model']['objects'])==1 and state['model']['objects'][0]['shape']=='triangle',state
        tap('Text placement');tap('Enter text…');tap('Text to place')
        adb('shell','input','text','Toolbar-text')
        tap('Continue');canvas_gesture()
        state=instrument('state')
        assert state['model']['objects'][-1]['text']=='Toolbar-text',state
        print('PASS shape palette and text dialog through real Android UI input',flush=True)
        instrument('configure',document='drawing-tools',host='')
        for shape in ('rect','ellipse','arrow','line','triangle','diamond','pentagon','hexagon','star'):
            state=instrument('shape',shape=shape)
            assert state['model']['objects'][-1]['shape']==shape,state
        before=len(state['model']['objects'])
        state=instrument('shape',shape='triangle',cancel='true')
        assert len(state['model']['objects'])==before,'Canceled gesture was persisted'
        state=instrument('shape',shape='line',x=400,y=300,w=-120,h=-80)
        line=state['model']['objects'][-1]
        assert all(abs(line[key]-expected)<.001 for key,expected in zip(('x','y','w','h'),(400,300,-120,-80))),line
        state=instrument('undo');assert state['model']['objectActive'][line['object']] is False
        state=instrument('redo');assert state['model']['objectActive'][line['object']] is True
        state=instrument('text',text='Editable text')
        text=state['model']['objects'][-1]
        assert text['shape']=='text' and text['text']=='Editable text' and text['fontSize']==32,text
        saved=read_replica(SERIAL,'drawing-tools')
        actions=[op for op in saved['pending'] if op['kind']=='batch']
        assert len(actions)==13,(len(actions),actions)  # 10 shapes + undo + redo + text
        assert all(op['actor']=='human' for op in actions),actions
        tablet('object','update',text['object'],'-',value={'text':'CLI edits Android text'})
        state=instrument('state')
        assert state['model']['objects'][-1]['text']=='CLI edits Android text'
        bridge.close();adb('shell','am','force-stop','com.notemaker')
        state=instrument('state')
        assert len(state['model']['objects'])==11
        print('PASS Android shape/text creation, cancellation, signed lines, atomic history and offline restart',flush=True)
        # Relaunch while another process still owns proxy.lock, as a killed
        # predecessor does while it exits (ADR 0031): the app waits for it.
        bridge.close();adb('shell','am','force-stop','com.notemaker')
        holder=subprocess.Popen(cli_command(SERIAL)+['--state','/data/user/0/com.notemaker/files','--document','drawing-tools','serve','8799'],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        assert 'descriptor' in holder.stdout.readline()
        released=[]
        def release():
            # Let go 1 s after the relaunched app process appears.
            deadline=time.monotonic()+30
            while time.monotonic()<deadline and not subprocess.run(['adb','-s',SERIAL,'shell','pidof','com.notemaker'],capture_output=True,text=True).stdout.strip():
                time.sleep(.1)
            time.sleep(1)
            released.append(holder.poll() is None)
            adb('shell','run-as','com.notemaker','pkill','-f','libnotemaker_cli');holder.wait(10)
        threading.Thread(target=release,daemon=True).start()
        state=instrument('state')
        assert released==[True],released
        assert len(state['model']['objects'])==11
        print('PASS relaunch waits for the previous replica owner to exit',flush=True)
        instrument('configure',document='drawing-order',host='')
        state=instrument('mixed-drawing')
        objects=state['model']['objects'];strokes=state['model']['strokes']
        assert len(objects)==len(strokes)==2,state
        assert all(state['model']['objectActive'].get(o['object']) is not False for o in objects)
        assert all(state['model']['strokeActive'].get(s['stroke']) is not False for s in strokes)
        for key,entity,identity in [('strokeActive',strokes[1],'stroke'),('objectActive',objects[1],'object'),
                                    ('strokeActive',strokes[0],'stroke'),('objectActive',objects[0],'object')]:
            state=instrument('undo')
            assert state['model'][key].get(entity[identity]) is False,('Undo order',key,entity,state)
        print('PASS rapid interleaved shape/pen actions preserve undo order and block pending undo',flush=True)
        instrument('configure',document='integration',host=f'http://10.0.2.2:{PORT}')
        state=tablet('diagram','add','-',value=EXAMPLE,concurrent=True)
        assert len(state['model']['objects'])==8 and state['strokes']==1 and state['pending']>0,state
        print('PASS app-spawned CLI and pen input share local state offline',flush=True)
        tablet('object','update','app','-',value={'scope':{'layer':'architecture','page':0},'x':125,'text':'Offline agent edit'})
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
        for attempt in range(2):
            tap('Document actions');tap('Edit diagram objects…')
            try:
                tap('rect · Offline agent edit')
                break
            except AssertionError:
                if attempt:
                    raise
        (OUT/'object-editor.png').write_bytes(subprocess.check_output(['adb','-s',SERIAL,'exec-out','screencap','-p']))
        tap('Erase')
        state=tablet('inspect');assert state['model']['objectActive']['app'] is False
        tap('More tools');tap('Undo and redo');tap('Undo')
        state=tablet('inspect');assert state['model']['objectActive']['app'] is True
        print('PASS object editor erasure and human undo visible to CLI',flush=True)
        # Reset only the reserved desktop test replica by assigning a fresh state directory.
        desktop_proxy=Proxy(Path(temp)/'desktop','integration')
        server=subprocess.Popen(['python3',str(ROOT/'apps/server/server.py'),'--host','127.0.0.1','--port',str(PORT),'--database',temp+'/server.sqlite3'],stdout=(OUT/'server.log').open('w'),stderr=subprocess.STDOUT)
        wait(lambda:urllib.request.urlopen(f'http://127.0.0.1:{PORT}/api/info',timeout=2).status==200)
        wait(lambda:not replica()['pending'])
        desktop('sync',f'http://127.0.0.1:{PORT}')
        assert len(desktop('inspect')['objects'])==9
        desktop('object','update','cli','-',value={'scope':{'layer':'architecture','page':0},'text':'Desktop agent','x':580})
        desktop('layer','put','-',value={'layer':'test-layer','name':'Forced layer','force':True})
        desktop('sync',f'http://127.0.0.1:{PORT}')
        wait(lambda:object_snapshot('cli').get('text')=='Desktop agent')
        print('PASS desktop CLI ↔ server ↔ Android local state convergence',flush=True)
        server.terminate();server.wait(timeout=10);server=None
        tablet('object','hide','app','architecture','0')
        desktop('object','update','app','-',value={'scope':{'layer':'architecture','page':0},'x':250})
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
        bridge.close();adb('shell','am','force-stop','com.notemaker')
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
        tablet('object','update','cli','-',value={'scope':{'layer':'architecture','page':0},'text':'Selected proxy'})
        assert next(o for o in desktop('inspect')['objects'] if o['object']=='cli')['text']=='Selected proxy'
        desktop('object','update','cli','-',value={'scope':{'layer':'architecture','page':0},'text':'SSE update'})
        wait(lambda:next(o for o in instrument('state')['model']['objects'] if o['object']=='cli')['text']=='SSE update')
        print('PASS app and spawned CLI use selected proxy; SSE refreshes remote editor',flush=True)
        state=instrument('configure',document='remote-pages',type='pages',proxyUrl=f'http://127.0.0.1:{desktop_proxy.port}',proxyToken=desktop_proxy.descriptor['token'])
        assert state['type']=='pages',state
        instrument('page',page=1)
        state=instrument('draw')
        assert state['model']['strokes'][0]['page']==1,state
        state=instrument('shape',shape='star')
        star=state['model']['objects'][-1]
        assert star['shape']=='star' and star['page']==1,star
        p=subprocess.run([str(CLI),'--proxy-file',str(desktop_proxy.descriptor_path),'--document','remote-pages','inspect'],text=True,capture_output=True,timeout=30)
        assert p.returncode==0,p.stderr
        assert json.loads(p.stdout)['objects'][-1]==star,'Android shape bypassed selected proxy'
        print('PASS selected proxy creates and renders a multipage document',flush=True)
        bridge.close();adb('emu','kill');emulator.wait(timeout=20);emulator=None
finally:
    bridge.close()
    if desktop_proxy:desktop_proxy.stop()
    if server:server.terminate();server.wait(timeout=10)
    if emulator:
        try:(OUT/'failure-logcat.txt').write_text(adb('logcat','-d','-s','NoteMaker','AndroidRuntime'))
        except Exception:pass
        try:adb('emu','kill')
        except Exception:emulator.terminate()
        try:emulator.wait(timeout=20)
        except subprocess.TimeoutExpired:emulator.kill()

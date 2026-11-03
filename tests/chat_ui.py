#!/usr/bin/env python3
"""Agent popup on an isolated API 36 emulator with the real mock chat service."""
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET
from chat_checks import ChatChecks, ROOT
from android_bridge import AndroidBridge

OUT = ROOT / '.test-artifacts/chat-ui'
OUT.mkdir(parents=True, exist_ok=True)
SERIAL = 'emulator-5586'
bridge = AndroidBridge(SERIAL, OUT)


def command(*args, timeout=90):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, timeout=timeout)


def adb(*args):
    return command('adb', '-s', SERIAL, *args)


def wait(fn, seconds=90):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except Exception as error:
            last = error
        time.sleep(1)
    raise AssertionError(f'Timed out: {last}')


def tree():
    root=ET.Element('hierarchy')
    for node in bridge.ui_nodes():
        ET.SubElement(root,'node',{key:str(value) for key,value in node.items()})
    return root


def tap(value):
    # Tap only once the control is in the same place in two consecutive UI
    # dumps, so a dialog or popup still opening cannot move it from under us.
    deadline = time.monotonic() + 15
    previous = None
    while True:
        nodes = tree()
        node = next((n for n in nodes.iter('node') if n.attrib.get('content-desc') == value), None)
        if node is None:
            node = next((n for n in nodes.iter('node') if n.attrib.get('text', '').casefold() == value.casefold()), None)
        if node is not None and previous is not None and node.attrib['bounds'] == previous:
            break
        previous = None if node is None else node.attrib['bounds']
        if time.monotonic() >= deadline:
            node = None
            break
        time.sleep(.2)
    if node is None:
        (OUT / 'missing.xml').write_bytes(ET.tostring(nodes))
        raise AssertionError('Missing ' + value)
    left, top, right, bottom = map(int, re.findall(r'\d+', node.attrib['bounds']))
    adb('shell', 'input', 'tap', str((left + right)//2), str((top + bottom)//2))
    time.sleep(.3)


def capture(name):
    (OUT / (name + '.png')).write_bytes(subprocess.check_output(['adb', '-s', SERIAL, 'exec-out', 'screencap', '-p']))


def control_bounds(label):
    deadline=time.monotonic()+15
    node=None
    while node is None and time.monotonic()<deadline:
        node=next((n for n in tree().iter('node') if n.get('content-desc') == label), None)
        if node is None:time.sleep(.2)
    if node is None:raise AssertionError('Missing control '+label)
    return list(map(int, re.findall(r'\d+', node.get('bounds'))))


def keyboard_top():
    dump = adb('shell', 'dumpsys', 'window')
    (OUT / 'keyboard-window.txt').write_text(dump)
    for line in dump.splitlines():
        if re.search(r'(?:type|mType)=ime\b', line) and re.search(r'(?:visible|mVisible)=true', line):
            frame = re.search(r'(?:frame|mFrame)=\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]', line)
            if frame:
                return int(frame.group(2))
    return None


check = ChatChecks()
emulator = None
try:
    assert SERIAL not in command('adb', 'devices'), 'Reserved emulator already running'
    check.setUp()
    for _ in range(6):
        check.create()
    with tempfile.TemporaryDirectory(prefix='notemaker-chat-ui-') as temp:
        env = dict(os.environ, ANDROID_AVD_HOME=temp + '/avd')
        emulator = subprocess.Popen(['boox-emulator-headless', '-port', '5586', '-memory', '1536', '-cores', '2', '-gpu', os.environ.get('NOTEMAKER_TEST_GPU', 'host'), '-feature', '-Vulkan', '-wipe-data'],
            env=env, stdout=(OUT / 'emulator.log').open('w'), stderr=subprocess.STDOUT)
        wait(lambda: adb('shell', 'getprop', 'sys.boot_completed').strip() == '1', 240)
        for setting in ('window_animation_scale', 'transition_animation_scale', 'animator_duration_scale'):
            adb('shell', 'settings', 'put', 'global', setting, '0')  # taps must not land on animating dialogs
        adb('install', '-r', str(ROOT / 'apps/android/build/notemaker.apk'))
        adb('shell', 'settings', 'put', 'secure', 'show_ime_with_hard_keyboard', '1')
        bridge.command('configure',document='chat-ui',host=f'http://10.0.2.2:{check.port}')
        bridge.close()
        adb('shell', 'am', 'force-stop', 'com.notemaker')
        adb('shell', 'am', 'start', '-W', '-n', 'com.notemaker/.MainActivity')
        time.sleep(2)
        tap('Open Agent'); capture('recent')
        wait(lambda: sum('New chat · idle' == n.attrib.get('text') for n in tree().iter('node')) == 5)
        tap('More chats'); wait(lambda: sum('New chat · idle' == n.attrib.get('text') for n in tree().iter('node')) == 6)
        tap('New chat'); tap('Harness'); tap('mock')
        tap('Model'); tap('openai / gpt-5.6-luna')
        tap('Effort'); tap('high'); capture('new-chat')
        tap('Start chat')
        original_bottom = control_bounds('Send')[3]
        tap('Message agent'); adb('shell', 'input', 'text', 'Hello-agent')
        top = wait(keyboard_top)
        assert control_bounds('Message agent')[3] <= top
        assert control_bounds('Send')[3] <= top
        assert control_bounds('Send')[3] < original_bottom
        capture('keyboard-visible')
        tap('Send')  # Sending must work with the keyboard still open.
        if keyboard_top() is not None:
            adb('shell', 'input', 'keyevent', '4')
        wait(lambda: control_bounds('Send')[3] == original_bottom)
        print('PASS keyboard-visible composer, Send, and restored popup size', flush=True)
        chat = check.get('/api/chats')['chats'][0]['id']
        config = check.get('/api/chats/' + chat)
        assert config['model'] == 'gpt-5.6-luna' and config['effort'] == 'high', config
        assert check.wait_done(chat)['status'] == 'done'
        with sqlite3.connect(check.data / 'chat.sqlite3') as db:
            sent = json.loads(db.execute('SELECT payload FROM chat_requests WHERE chat=?', (chat,)).fetchone()[0])
        app_session = sent['session']
        assert str(uuid.UUID(app_session)) == app_session, sent
        assert sent['document'] == 'chat-ui' and sent['text'] == 'Hello-agent', sent
        sessions = check.get('/api/ui/sessions')['sessions']
        assert any(row['session'] == app_session for row in sessions), sessions
        wait(lambda: any('Mock response: Hello-agent' in n.attrib.get('text', '') for n in tree().iter('node')))
        tap('Expand tool activity'); capture('conversation')
        wait(lambda: any('no document was read or changed' in n.attrib.get('text', '') for n in tree().iter('node')))
        tap('Close'); tap('Open Agent'); tap('Hello-agent · done')
        wait(lambda: any('Mock response: Hello-agent' in n.attrib.get('text', '') for n in tree().iter('node')))
        tap('Message agent'); adb('shell', 'input', 'text', 'Saved-draft'); adb('shell', 'input', 'keyevent', '4')
        tap('Close'); tap('Open Agent'); tap('Hello-agent · done')
        wait(lambda: any(n.attrib.get('text') == 'Saved-draft' for n in tree().iter('node')))
        capture('reopened')
        tap('Close')
        # Replay representative Pi events without spending provider tokens. Only
        # the temporary chat journal is seeded; no document data is touched.
        tool_chat = check.create()
        fixtures = [
            ('read', {'path':'apps/cli/README.md', 'offset':10, 'limit':20}, 'Read result'),
            ('bash', {'command':'printf "hello\\n"', 'timeout':30}, 'hello'),
            ('edit', {'path':'scratch.txt', 'oldText':'before', 'newText':'after'}, 'Edited'),
        ]
        with sqlite3.connect(check.data / 'chat.sqlite3') as db:
            db.execute('UPDATE chats SET title=? WHERE id=?', ('Tool examples', tool_chat))
            for name, args, output in fixtures:
                event = dict(kind='tool', id=name, name=name, args=json.dumps(args), status='done', text=output)
                db.execute('INSERT INTO chat_events(chat,payload) VALUES(?,?)', (tool_chat,json.dumps(event)))
        tap('Open Agent'); tap('Refresh')
        wait(lambda: any(n.get('text') == 'Tool examples · idle' for n in tree().iter('node')))
        tap('Tool examples · idle')
        wait(lambda: any('read apps/cli/README.md' in n.get('text','') for n in tree().iter('node')))
        tap('▸ read apps/cli/README.md · done')
        wait(lambda: any('Starting line: 10' in n.get('text','') and 'Read result' in n.get('text','') for n in tree().iter('node')))
        tap('▸ bash printf "hello\\n" · done')
        wait(lambda: any('printf "hello\\n"\n\nTimeout (seconds): 30' in n.get('text','') for n in tree().iter('node')))
        tap('▸ edit scratch.txt · done')
        wait(lambda: any('Replace:\nbefore' in n.get('text','') and 'With:\nafter' in n.get('text','') for n in tree().iter('node')))
        capture('tool-formatting')
        process = adb('shell', 'pidof', 'com.notemaker').strip()
        errors = adb('logcat', '-d', '--pid=' + process, '-s', 'AndroidRuntime:E')
        assert 'FATAL EXCEPTION' not in errors, errors
        print('PASS: chats, selectors, streaming, draft persistence, keyboard resize/restore and readable read/bash/edit activity')
        bridge.close();adb('emu', 'kill'); emulator.wait(timeout=30); emulator = None
finally:
    bridge.close()
    if emulator:
        emulator.terminate(); emulator.wait(timeout=30)
    if hasattr(check, 'process'):
        check.tearDown()

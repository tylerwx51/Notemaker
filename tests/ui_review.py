#!/usr/bin/env python3
"""Exercise the editor chrome on an already running emulator; save review screenshots."""
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / '.test-artifacts' / 'ui-review'
OUT.mkdir(parents=True, exist_ok=True)
SERIAL = sys.argv[1] if len(sys.argv) > 1 else 'emulator-5580'
DOCUMENT = 'Interface-review'
FILE = 'files/document-' + str(uuid.UUID(bytes=hashlib.md5(DOCUMENT.encode()).digest(), version=3)) + '.json'

def adb(*args):
    return subprocess.check_output(['adb', '-s', SERIAL, *args], text=True, stderr=subprocess.STDOUT, timeout=90)

def tree():
    adb('shell', 'uiautomator', 'dump', '/data/local/tmp/notemaker-ui.xml')
    return ET.fromstring(adb('shell', 'cat', '/data/local/tmp/notemaker-ui.xml'))

def find(value, attribute='content-desc'):
    nodes = tree()
    for node in nodes.iter('node'):
        actual = node.attrib.get(attribute, '')
        if actual == value or (attribute == 'text' and actual.casefold() == value.casefold()):
            return node
    (OUT / 'missing-control.xml').write_bytes(ET.tostring(nodes))
    raise AssertionError(f'Missing {attribute}: {value}')

def bounds(node):
    return list(map(int, re.findall(r'\d+', node.attrib['bounds'])))

def tap(value, attribute='content-desc'):
    left, top, right, bottom = bounds(find(value, attribute))
    adb('shell', 'input', 'tap', str((left + right)//2), str((top + bottom)//2))
    time.sleep(.25)

def capture(name):
    with (OUT / (name + '.png')).open('wb') as output:
        subprocess.check_call(['adb', '-s', SERIAL, 'exec-out', 'screencap', '-p'], stdout=output)

def state():
    return json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', FILE))

def operations():
    saved = state()
    return saved['events'] + saved['pending']

def strokes():
    return {op['stroke'] for op in operations() if op['kind'] == 'stroke'}

def layer(name):
    return next(op['layer'] for op in reversed(operations()) if op['kind'] == 'layer' and op['name'] == name)

def draw():
    left, top, right, bottom = bounds(find('Drawing canvas'))
    # Keep the gesture clear of the expanded tray.
    adb('shell', 'input', 'swipe', str(left+100), str(top+260), str(left+450), str(top+450), '400')
    time.sleep(.4)

def create_layer(name):
    tap('＋  New layer')
    tap('android.widget.EditText', 'class')
    adb('shell', 'input', 'text', name)
    tap('Create', 'text')

adb('wait-for-device')
for _ in range(180):
    if adb('shell', 'getprop', 'sys.boot_completed').strip() == '1':
        break
    time.sleep(1)
else:
    raise AssertionError('Emulator did not finish booting')
adb('install', '-r', str(ROOT / 'apps/android/build/notemaker.apk'))
adb('shell', 'am', 'force-stop', 'com.notemaker')
adb('shell', 'run-as', 'com.notemaker', 'rm', '-f', FILE)
result = adb('shell', 'am', 'instrument', '-w', '-e', 'action', 'configure', '-e', 'document', DOCUMENT,
             '-e', 'host', 'http://10.0.2.2:18766', 'com.notemaker/.Integration')
assert 'PASS configure' in result, result
adb('shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
time.sleep(2)
if any(n.attrib.get('content-desc') == 'Expand layer tray' for n in tree().iter('node')):
    tap('Expand layer tray')

canvas_bounds = bounds(find('Drawing canvas'))
tap('Pen tools')
tap('Blue ink')
capture('pen-popup')
tap('Done')
draw()
assert any(o['kind'] == 'stroke' and o['color'] == -14393923 for o in operations()), 'Blue ink was not applied'
create_layer('Guides')
draw()
create_layer('Annotations')
draw()
tap('Hide layer Guides')
assert layer('Guides') in state()['hidden']
find('Hidden', 'text')
capture('expanded-layers')

tap('Options for layer Guides')
capture('layer-popup')
tap('Force visible for everyone')
find('Forced visible layer Guides')
assert layer('Guides') in state()['hidden'], 'Force should preserve the local hiding preference'
tap('Options for layer Guides')
tap('Stop forcing visibility')
find('Show layer Guides')

tap('Collapse layer tray')
assert bounds(find('Drawing canvas')) == canvas_bounds, 'Collapsing the tray moved the canvas'
find('Show layer Guides')
capture('collapsed-layers')
tap('Show layer Guides')
assert layer('Guides') not in state()['hidden']
tap('Expand layer tray')
assert bounds(find('Drawing canvas')) == canvas_bounds, 'Expanding the tray moved the canvas'

# The tray's blank area must consume touches instead of drawing underneath it.
before = strokes()
left, top, right, bottom = bounds(find('＋  New layer'))
adb('shell', 'input', 'tap', str((left+right)//2), str(canvas_bounds[3]-220))
assert strokes() == before, 'A tray tap leaked through to the drawing surface'

tap('Undo and redo')
tap('Undo')
assert [o for o in operations() if o['kind'] == 'active'][-1]['active'] is False
tap('Undo and redo')
tap('Redo')
assert [o for o in operations() if o['kind'] == 'active'][-1]['active'] is True

tap('Canvas navigation')
tap('Return to pen')
tap('Eraser tools')
find('Stroke eraser', 'text')
tap('Ready to erase')
tap('Pen tools')
tap('Done')
tap('Page actions')
find('Single page', 'text')
adb('shell', 'input', 'keyevent', '4')
tap('Document actions')
find('Open or create document…')
adb('shell', 'input', 'keyevent', '4')

# Keep one hidden layer in the final review image.
tap('Hide layer Guides')
capture('editor-final')
# User preference should survive process restart.
tap('Collapse layer tray')
adb('shell', 'am', 'force-stop', 'com.notemaker')
adb('shell', 'am', 'start', '-n', 'com.notemaker/.MainActivity')
time.sleep(2)
find('Expand layer tray')
tap('Expand layer tray')
print('PASS tool popups, colors, layers, visibility, force visibility, undo/redo, tray touch isolation, stable canvas bounds, and persisted tray preference', flush=True)

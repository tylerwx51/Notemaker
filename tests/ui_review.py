#!/usr/bin/env python3
"""Exercise anchored editor menus on a running emulator; retain screenshots and test notes."""
import pathlib
import re
import subprocess
import sys
import time
import uuid
from replica import read_replica
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / '.test-artifacts' / 'ui-review'
OUT.mkdir(parents=True, exist_ok=True)
SERIAL = sys.argv[1] if len(sys.argv) > 1 else 'emulator-5580'
if not SERIAL.startswith('emulator-'):
    raise SystemExit('Use an emulator serial; this review changes test documents and display size.')
DOCUMENT = 'Interface-review-' + uuid.uuid4().hex[:8]

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
    return read_replica(SERIAL, DOCUMENT)

def operations():
    saved = state()
    return saved['events'] + saved['pending']

def strokes():
    return {op['stroke'] for op in operations() if op['kind'] == 'stroke'}

def layer(name):
    return next(op['layer'] for op in reversed(operations()) if op['kind'] == 'layer' and op['name'] == name)

def draw():
    left, top, right, bottom = bounds(find('Drawing canvas'))
    # Draw in the upper-left of the canvas, away from system navigation.
    adb('shell', 'input', 'swipe', str(left+100), str(top+260), str(left+450), str(top+450), '400')
    time.sleep(.4)

def create_layer(name):
    tap('Layer actions')
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
result = adb('shell', 'am', 'instrument', '-w', '-e', 'action', 'configure', '-e', 'document', DOCUMENT,
             '-e', 'host', 'http://10.0.2.2:18766', 'com.notemaker/.Integration')
assert 'PASS configure' in result, result
adb('shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
time.sleep(2)
canvas_bounds = bounds(find('Drawing canvas'))
capture('editor-empty')
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
tap('Layer actions')
tap('Hide layer Guides')
assert layer('Guides') in state()['hidden']
find('Hidden', 'text')
capture('layers-popup')

tap('Options for layer Guides')
capture('layer-popup')
tap('Force visible for everyone')
tap('Layer actions')
find('Forced visible layer Guides')
assert layer('Guides') in state()['hidden'], 'Force should preserve the local hiding preference'
tap('Options for layer Guides')
tap('Stop forcing visibility')
tap('Layer actions')
find('Show layer Guides')
tap('Show layer Guides')
assert layer('Guides') not in state()['hidden']
# A panel header must consume taps without drawing on the canvas behind it.
before = strokes()
tap('Layers', 'text')
assert strokes() == before, 'A panel tap leaked through to the drawing surface'
adb('shell', 'input', 'keyevent', '4')
assert bounds(find('Drawing canvas')) == canvas_bounds, 'Opening Layers moved the canvas'

tap('Undo')
assert [o for o in operations() if o['kind'] == 'active'][-1]['active'] is False
tap('Redo')
assert [o for o in operations() if o['kind'] == 'active'][-1]['active'] is True

tap('Canvas navigation')
tap('Return to pen')
tap('Eraser tools')
find('Stroke erase')
capture('eraser-popup')
tap('Ready to erase')
tap('Pen tools')
tap('Done')
tap('Page actions')
find('Single page', 'text')
adb('shell', 'input', 'keyevent', '4')

tap('Insert content')
find('Insert picture…')
find('Insert SVG artwork…')
capture('insert-popup')
adb('shell', 'input', 'keyevent', '4')
tap('Page templates')
find('Dot grid')
capture('templates-popup')
adb('shell', 'input', 'keyevent', '4')
tap('Shape tools')
capture('shapes-popup')
tap('Create Rectangle')
draw()
shape_batches = [o for o in operations() if o['kind'] == 'batch' and any(
    a['kind'] == 'object' and a.get('shape') == 'rect' for a in o['actions'])]
assert len(shape_batches) == 1, 'Shape placement must save one atomic rectangle action'
tap('Pen tools')
tap('Done')
tap('Document actions')
find('Open or create document…')
adb('shell', 'input', 'keyevent', '4')

# Keep one hidden layer in the final review image.
tap('Layer actions')
tap('Hide layer Guides')
adb('shell', 'input', 'keyevent', '4')
capture('editor-final')
# Label preference should survive restart; icons keep accessible descriptions.
tap('More tools')
capture('more-popup')
tap('Toolbar preferences…')
tap('Show tool labels', 'text')
adb('shell', 'input', 'keyevent', '4')
adb('shell', 'am', 'force-stop', 'com.notemaker')
adb('shell', 'am', 'start', '-n', 'com.notemaker/.MainActivity')
time.sleep(2)
find('Layer actions')
assert not any(n.get('text') == 'Layers' for n in tree().iter('node')), 'Label preference was not retained'
tap('More tools')
tap('Toolbar preferences…')
tap('Show tool labels', 'text')
adb('shell', 'input', 'keyevent', '4')

# A short, narrow viewport must still allow access to the last menu action.
try:
    adb('shell', 'wm', 'size', '1080x1920')
    adb('shell', 'wm', 'density', '420')
    time.sleep(2)
    tap('Pen tools')
    capture('narrow-pen-popup')
    for _ in range(5):
        nodes = tree()
        done = next((n for n in nodes.iter('node') if n.get('content-desc') == 'Done'), None)
        if done is not None and bounds(done)[3] > bounds(done)[1]:
            tap('Done')
            break
        adb('shell', 'input', 'swipe', '600', '1550', '600', '650', '350')
    else:
        raise AssertionError('Bottom of narrow popup is unreachable')
    tap('Shape tools')
    find('Create Rectangle')
    capture('narrow-shapes-popup')
    adb('shell', 'input', 'keyevent', '4')
    adb('shell', 'input', 'swipe', '70', '1530', '70', '550', '350')
    tap('More tools')
    find('Toolbar preferences…')
    capture('narrow-more-popup')
    adb('shell', 'input', 'keyevent', '4')
finally:
    adb('shell', 'wm', 'size', 'reset')
    adb('shell', 'wm', 'density', 'reset')
print('PASS anchored menus, colors, shape creation, layer visibility, force visibility, undo/redo, panel touch isolation, stable canvas bounds, persisted labels, and narrow-screen scrolling', flush=True)

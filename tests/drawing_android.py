#!/usr/bin/env python3
"""Exercise the Android enclosed-area fill gesture on an isolated emulator."""
import json
import base64
import os
from pathlib import Path
import shlex
import re
import xml.etree.ElementTree as ET
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
APK = ROOT / 'apps/android/build/notemaker.apk'
SERIAL = 'emulator-5590'
OUT = ROOT / '.test-artifacts/drawing-android'
OUT.mkdir(parents=True, exist_ok=True)


def run(*args, timeout=90):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, timeout=timeout)


def adb(*args):
    return run('adb', '-s', SERIAL, *args)


def wait_ready():
    deadline = time.monotonic() + 240
    while time.monotonic() < deadline:
        try:
            if adb('shell', 'getprop', 'sys.boot_completed').strip() == '1':
                return
        except Exception:
            pass
        time.sleep(1)
    raise TimeoutError('Android emulator did not finish booting')


def encoded(value):
    return base64.b64encode(json.dumps(value).encode()).decode()


def instrument(action, **kwargs):
    args = ['shell', 'am', 'instrument', '-w', '-e', 'action', action]
    for key, value in kwargs.items():
        args += ['-e', key, str(value)]
    result = run('adb', '-s', SERIAL, 'shell', shlex.join(args[1:] + ['com.notemaker/.Integration']))
    (OUT / f'{action}.txt').write_text(result)
    assert f'PASS {action}' in result, result
    if action != 'select-panel':
        run('adb', '-s', SERIAL, 'shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
    return result


def tap(label):
    hierarchy = ui_dump()
    tree = ET.fromstring(hierarchy)
    node = next((n for n in tree.iter('node') if n.attrib.get('content-desc') == label
                 or label.casefold() in n.attrib.get('text', '').casefold()), None)
    if node is None:
        (OUT / f'missing-{re.sub(r"[^a-z0-9]+", "-", label.casefold()).strip("-")}.xml').write_text(hierarchy)
        screenshot(f'missing-{re.sub(r"[^a-z0-9]+", "-", label.casefold()).strip("-")}')
        raise AssertionError(f'Missing UI control {label}')
    bounds = list(map(int, re.findall(r'\d+', node.attrib['bounds'])))
    adb('shell', 'input', 'tap', str((bounds[0] + bounds[2]) // 2), str((bounds[1] + bounds[3]) // 2))
    time.sleep(.4)


def screenshot(name):
    (OUT / f'{name}.png').write_bytes(subprocess.check_output(['adb', '-s', SERIAL, 'exec-out', 'screencap', '-p']))


def latest_state():
    instrument('state')
    return json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))


def ui_dump():
    for attempt in range(8):
        try:
            adb('shell', 'uiautomator', 'dump', '/data/local/tmp/drawing-ui.xml')
            return adb('shell', 'cat', '/data/local/tmp/drawing-ui.xml')
        except subprocess.CalledProcessError:
            if attempt == 7:
                raise
            time.sleep(.5)


def show_editor():
    # Instrumentation teardown can race the immediate Activity relaunch.
    for _ in range(5):
        adb('shell','am','start','-W','-n','com.notemaker/.MainActivity')
        hierarchy=ui_dump()
        if 'content-desc="Pen tools"' in hierarchy:
            return
        if 'package="com.notemaker"' in hierarchy:
            adb('shell','input','keyevent','4')
        time.sleep(.3)
    raise AssertionError('Editor did not return after instrumentation')


def check_panels():
    show_editor()
    tap('Pen tools')
    tap('More brush types…')
    adb('shell', 'uiautomator', 'dump', '/data/local/tmp/drawing-ui.xml')
    assert 'charcoal' in adb('shell', 'cat', '/data/local/tmp/drawing-ui.xml').casefold()
    tap('charcoal')
    tap('Erase')
    tap('Scribble to erase')
    adb('shell', 'input', 'keyevent', '4')
    prefs = adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
    assert 'name="scribbleToErase" value="true"' in prefs, prefs
    tap('Page templates')
    tap('Ruled')
    time.sleep(1)
    state = latest_state()
    assert any(o['shape'] == 'svg' and o.get('layer') == 'template-0' for o in state['model']['objects']), state
    instrument('layer', layer='ui-layer')
    show_editor()
    tap('Layer actions')
    tap('Options for layer Test layer')
    layer_menu = ui_dump()
    for option in ('Rename layer…', 'Duplicate layer', 'Move layer up', 'Merge into another layer…', 'Archive layer'):
        assert option in layer_menu, (option, layer_menu)
    tap('Duplicate layer')
    time.sleep(1)
    state = latest_state()
    assert any(layer['layer'] != 'ui-layer' and layer['name'] == 'Test layer copy' for layer in state['model']['layers']), state
    show_editor()
    tap('More tools')
    tap('Toolbar preferences…')
    tap('Show tool labels')
    prefs = adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
    assert 'name="showToolLabels" value="false"' in prefs, prefs
    print('PASS Android brush, eraser, template, layer actions, and toolbar preferences panels')


def main():
    devices = run('adb', 'devices')
    assert SERIAL not in devices, f'Reserved emulator already running: {SERIAL}'
    emulator = None
    with tempfile.TemporaryDirectory(prefix='notemaker-drawing-android-') as temp:
        env = dict(os.environ, ANDROID_AVD_HOME=temp + '/avd')
        log = (OUT / 'emulator.log').open('w')
        try:
            emulator = subprocess.Popen(
                ['boox-emulator-headless', '-port', '5590', '-memory', '1536', '-cores', '2', '-wipe-data'],
                env=env, stdout=log, stderr=subprocess.STDOUT)
            wait_ready()
            adb('install', '-r', str(APK))
            if '--coverage-only' in sys.argv or '--panels-only' in sys.argv:
                instrument('configure', document='android-coverage', host='offline')
                if '--coverage-only' in sys.argv:
                    instrument('coverage')
                    print('PASS Android coverage, masks, pixels and layer switching')
                if '--panels-only' in sys.argv:
                    check_panels()
                return
            if '--tool-switch-only' in sys.argv:
                instrument('configure', document='android-tool-switch-selection', host='offline')
                instrument('tool-switch-selection')
                print('PASS switching tools releases active and late-acquired selection leases and cancels previews')
                return
            instrument('configure', document='android-coverage', host='offline')
            instrument('coverage')
            print('PASS Android coverage, masks, pixels and layer switching')
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w',
                         '-e', 'action', 'configure', '-e', 'document', 'android-fill',
                         '-e', 'host', 'offline', 'com.notemaker/.Integration')
            assert 'PASS configure' in result, result
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w',
                         '-e', 'action', 'fill-tap', 'com.notemaker/.Integration')
            assert 'PASS fill-tap' in result, result
            state = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))
            objects = state['model']['objects']
            assert len(objects) == 2, objects
            assert objects[0]['shape'] == 'rect', objects
            assert objects[1]['shape'] == 'svg' and '#ff0000' in objects[1]['svg'], objects
            print('PASS Android tap-to-fill creates a single filled region object')
            instrument('configure', document='android-tool-switch-selection', host='offline')
            instrument('tool-switch-selection')
            print('PASS switching tools releases active and late-acquired selection leases and cancels previews')
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w',
                         '-e', 'action', 'configure', '-e', 'document', 'android-collaboration',
                         '-e', 'host', 'offline', 'com.notemaker/.Integration')
            assert 'PASS configure' in result, result
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w',
                         '-e', 'action', 'collaboration', 'com.notemaker/.Integration', timeout=90)
            assert 'PASS collaboration' in result, result
            state = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))
            model = state['model']
            original = next(o for o in model['objects'] if o['object'] == 'collaboration-object')
            assert original['rotation'] == 30 and original['flipX'] is True, original
            assert original['x'] == 100 and original['y'] == 140 and original['w'] == 240 and original['h'] == 180, original
            assert model['objectActive']['collaboration-object'] is False, model
            print('PASS Android selection lease, ephemeral preview/cancel, atomic rotate/flip, duplicate and delete')
            instrument('configure', document='android-remote-preview', host='offline')
            instrument('remote-preview')
            print('PASS Android foreign preview and cancellation update the canvas without durable edits')
            instrument('configure', document='android-undo-lock', host='offline')
            instrument('locked-undo')
            print('PASS Android locked undo preserves local history until the lease is released')
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w', '-e', 'action', 'configure',
                         '-e', 'document', 'android-locks', '-e', 'host', 'offline', 'com.notemaker/.Integration')
            assert 'PASS configure' in result, result
            instrument('shape', shape='rect')
            state = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))
            locked_id = state['model']['objects'][-1]['object']
            instrument('locked-selection', object=locked_id,
                       cli=encoded(['selection', 'select', '-']),
                       input=encoded({'session': '11111111-1111-4111-8111-111111111111', 'page': 0,
                                      'entities': [{'kind': 'object', 'id': locked_id}]}))
            print('PASS Android selection skips another actor’s leased object')
            result = run('adb', '-s', SERIAL, 'shell', 'am', 'instrument', '-w', '-e', 'action', 'configure',
                         '-e', 'document', 'android-lasso', '-e', 'host', 'offline', 'com.notemaker/.Integration')
            assert 'PASS configure' in result, result
            instrument('lasso')
            print('PASS Android drag-box selection and stroke-segment midpoint hit testing')
            instrument('configure', document='android-freeform-lasso', host='offline')
            instrument('freeform-lasso')
            print('PASS Android freeform lasso excludes objects outside the traced polygon')
            instrument('configure', document='android-layer-order', host='offline')
            instrument('layer-order')
            print('PASS Android layer reorder changes pixel compositing and page template stays beneath ink')
            tap('Shape tools')
            screenshot('shape-palette')
            instrument('select-panel')
            screenshot('selection-panel')
            instrument('configure', document='android-conflict-ui', host='offline')
            instrument('conflict-preview')
            tap('conflict(s)')
            tap('android-conflict-item')
            conflict_ui = ui_dump()
            for label in ('Version A', 'Version B', 'Use Version A', 'Use Version B', 'Keep both versions'):
                assert label in conflict_ui, (label, conflict_ui)
            screenshot('conflict-comparison')
            tap('Use Version B')
            time.sleep(1)
            instrument('cli', cli=encoded(['conflicts']))
            conflict_state = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/cli-result.json'))
            assert conflict_state['conflicts'] == [], conflict_state
            instrument('cli', cli=encoded(['--actor', 'human', 'undo']))
            instrument('cli', cli=encoded(['conflicts']))
            conflict_state = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/cli-result.json'))
            assert len(conflict_state['conflicts']) == 1, conflict_state
            print('PASS Android conflict comparison renders both versions and all resolution choices')
            instrument('configure', document='android-clipboard', host='offline')
            instrument('clipboard')
            tap('Select and transform objects')
            selection_ui = ui_dump()
            assert 'Paste copied selection' in selection_ui, selection_ui
            screenshot('clipboard-panel')
            before = len(json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))['model']['objects'])
            tap('Paste copied selection')
            time.sleep(1)
            after = latest_state()
            assert len(after['model']['objects']) == before + 1, after['model']['objects']
            adb('shell', 'am', 'force-stop', 'com.notemaker')
            run('adb', '-s', SERIAL, 'shell', 'am', 'start', '-n', 'com.notemaker/.MainActivity')
            time.sleep(1)
            tap('Select and transform objects')
            assert 'Paste copied selection' in ui_dump(), 'Clipboard was lost after app restart'
            instrument('configure', document='android-clipboard-b')
            tap('Select and transform objects')
            assert 'Paste copied selection' in ui_dump(), 'Cross-document clipboard was replaced by the destination document snapshot'
            tap('Paste copied selection')
            time.sleep(1)
            cross_doc = latest_state()
            assert any(o.get('shape') == 'rect' and o.get('x') == 144 for o in cross_doc['model']['objects']), cross_doc['model']['objects']
            print('PASS Android copy/paste survives app restart via local shared clipboard')
            check_panels()
        finally:
            if emulator is not None:
                try:
                    (OUT/'logcat.txt').write_text(adb('logcat','-d','-t','600'))
                except Exception:
                    pass
                emulator.terminate()
                try:
                    emulator.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    emulator.kill()
                    emulator.wait()
            log.close()


if __name__ == '__main__':
    main()

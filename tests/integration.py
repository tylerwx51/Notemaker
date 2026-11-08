#!/usr/bin/env python3
"""Launch two isolated API 36 emulators and exercise the production server/app."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.parse
import uuid
from replica import read_replica
from android_bridge import AndroidBridge
from android_emulator import Emulator

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.test-artifacts'
OUT.mkdir(exist_ok=True)
SERIALS = ['emulator-5580', 'emulator-5582']
emulators = {}
# Fresh AVDs: a killed emulator leaves its AVD locked for the next run.
avd_root = tempfile.TemporaryDirectory(prefix='notemaker-integration-')
server = None
completed = False
bridges = {serial: AndroidBridge(serial, OUT) for serial in SERIALS}

def command(args, **kwargs):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, **kwargs)

def adb(serial, *args):
    return command(['adb', '-s', serial, *args], timeout=90)

def wait_until(fn, seconds=90):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except Exception as error:
            last = error
        time.sleep(1)
    raise AssertionError(f'Timed out; last error: {last}')

def start_server():
    global server
    server = subprocess.Popen([sys.executable, str(ROOT / 'apps/server/server.py'), '--port', '18765', '--database', str(OUT / 'integration.sqlite3')], stdout=(OUT / 'server.log').open('a'), stderr=subprocess.STDOUT)
    def ready():
        if server.poll() is not None:
            raise RuntimeError('Integration server exited during startup; port 18765 may already be in use')
        return urllib.request.urlopen('http://127.0.0.1:18765/api/events?document=integration', timeout=2).status == 200
    wait_until(ready, 15)

def instrument(serial, action, **args):
    result = bridges[serial].command(action, **args)['stream']
    (OUT / f'{serial}-{action}.txt').write_text(result)
    assert f'PASS {action}' in result, result
    value = json.loads(adb(serial, 'shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))
    (OUT / f'{serial}-state.json').write_text(json.dumps(value, indent=2))
    return value

def state(serial):
    return json.loads(adb(serial, 'shell', 'run-as', 'com.notemaker', 'cat', 'files/integration.json'))

def snapshot(serial):
    instrument(serial, 'state', image='true')
    data = subprocess.check_output(['adb', '-s', serial, 'exec-out', 'run-as', 'com.notemaker', 'cat', 'files/integration.png'])
    (OUT / f'{serial}.png').write_bytes(data)
    return hashlib.sha256(data).hexdigest()

def events(document='integration'):
    return json.load(urllib.request.urlopen('http://127.0.0.1:18765/api/events?document=' + document))['events']

def converged(strokes=None):
    saved = []
    for serial in SERIALS:
        doc = json.loads((OUT / f'{serial}-state.json').read_text())['document']
        saved.append(read_replica(serial, doc))
    a, b = saved
    if a['pending'] or b['pending'] or a['cursor'] != b['cursor'] or a['events'] != b['events']:
        return False
    count = len({o['stroke'] for o in a['events'] if o['kind'] == 'stroke'})
    return (strokes is None or count == strokes) and a['cursor'] > 0

def same_image():
    a, b = [snapshot(s) for s in SERIALS]
    assert a == b, f'Android images differ: {a}, {b}'
    return a

def report(message):
    print(message, flush=True)
    with (OUT / 'results.txt').open('a') as f:
        f.write(message + '\n')

try:
    for serial in SERIALS:
        try:
            adb(serial, 'get-state')
        except Exception:
            continue
        raise RuntimeError(f'{serial} is already in use; stop it or choose different test ports')
    for name in ['integration.sqlite3', 'integration.sqlite3-wal', 'integration.sqlite3-shm', 'results.txt']:
        (OUT / name).unlink(missing_ok=True)
    start_server()
    for index, serial in enumerate(SERIALS):
        emulators[serial] = Emulator(5580 + index * 2, OUT / f'{serial}.log', Path(avd_root.name) / f'avd-{index}').start()
    for serial, emulator in emulators.items():
        report(f'Waiting for {serial} to boot')
        emulator.wait_booted()
        emulator.disable_animations()
        emulator.install()
        instrument(serial, 'configure', document='integration', host='http://10.0.2.2:18765')
    # Both actual emulators submit distinct pressure-bearing strokes concurrently.
    with concurrent.futures.ThreadPoolExecutor() as pool:
        futures = [pool.submit(instrument, s, 'draw', shift=i*120, color=[-16777216, -65536][i]) for i, s in enumerate(SERIALS)]
        for future in futures:
            future.result()
    wait_until(lambda: converged(2))
    image = same_image()
    report(f'PASS simultaneous stylus drawing: both logs and images match ({image})')
    # Idempotency: replay a committed operation through the real HTTP endpoint.
    before = len(events())
    operation = events()[0]['op']
    request = urllib.request.Request('http://127.0.0.1:18765/api/events', data=json.dumps({'document':'integration','operations':[operation]}).encode(), headers={'Content-Type':'application/json'})
    urllib.request.urlopen(request).read()
    assert len(events()) == before
    report('PASS repeated operation id does not duplicate events')
    instrument(SERIALS[0], 'layer')
    wait_until(converged)
    instrument(SERIALS[0], 'draw', shift=240, color=-16776961)
    wait_until(lambda: converged(3))
    instrument(SERIALS[1], 'hide')
    assert not state(SERIALS[1])['visible']
    assert snapshot(SERIALS[0]) != snapshot(SERIALS[1])
    report('PASS per-client visibility produces different images without changing shared strokes')
    instrument(SERIALS[0], 'layer', force='true')
    wait_until(converged)
    assert instrument(SERIALS[1], 'state')['visible']
    same_image()
    report('PASS force-visible layer overrides the other client’s local hiding')
    instrument(SERIALS[0], 'layer', force='false')
    wait_until(converged)
    assert not instrument(SERIALS[1], 'state')['visible']
    instrument(SERIALS[0], 'layer', force='true')
    wait_until(converged)
    same_image()
    report('PASS releasing force-visible restores the other client’s hiding preference')
    before_undo = snapshot(SERIALS[0])
    instrument(SERIALS[0], 'undo')
    wait_until(converged)
    assert snapshot(SERIALS[0]) != before_undo
    same_image()
    instrument(SERIALS[0], 'redo')
    wait_until(converged)
    assert snapshot(SERIALS[0]) == before_undo
    same_image()
    report('PASS undo/redo propagates to both clients and restores the image')
    instrument(SERIALS[0], 'erase', shift=240)
    wait_until(converged)
    assert snapshot(SERIALS[0]) != before_undo
    same_image()
    instrument(SERIALS[0], 'undo')
    wait_until(converged)
    assert snapshot(SERIALS[0]) == before_undo
    report('PASS stroke eraser and undo restore the shared image')
    # Stop the server while both clients keep drawing, then restart from SQLite.
    server.terminate();server.wait(timeout=10)
    with concurrent.futures.ThreadPoolExecutor() as pool:
        futures = [pool.submit(instrument, s, 'draw', shift=360+i*120) for i,s in enumerate(SERIALS)]
        for future in futures:
            assert future.result()['pending'] > 0
    bridges[SERIALS[0]].close();adb(SERIALS[0], 'shell', 'am', 'force-stop', 'com.notemaker')
    assert instrument(SERIALS[0], 'state')['pending'] > 0
    start_server()
    wait_until(lambda: converged(5))
    same_image()
    report('PASS offline drawing survives app restart; reconnect and server restart converge without lost strokes')
    from browser import check_browser
    browser_stats = check_browser(OUT, 18765, instrument(SERIALS[0], 'state', image='true')['cursor'])
    native_stats=json.loads(adb(SERIALS[0], 'shell', 'run-as', 'com.notemaker', 'cat', 'files/render-stats.json'))
    assert all(abs(a-b)<=2 for a,b in zip(native_stats['bounds'],browser_stats['bounds'])), (native_stats,browser_stats)
    assert abs(native_stats['pixels']-browser_stats['pixels'])/native_stats['pixels']<.10, (native_stats,browser_stats)
    report(f'PASS Chromium viewer replays the shared log and enforces forced visibility: {browser_stats}')
    for doc_type in ['pages', 'infinite']:
        for serial in SERIALS:
            instrument(serial, 'configure', document=doc_type, host='http://10.0.2.2:18765', type=doc_type)
        wait_until(converged)
        if doc_type == 'pages':
            for serial in SERIALS:
                instrument(serial, 'page', page=1)
        instrument(SERIALS[0], 'draw')
        wait_until(lambda: converged(1))
        assert all(instrument(s,'state')['type'] == doc_type for s in SERIALS)
        same_image()
        report(f'PASS {doc_type} document drawing and synchronization')
    instrument(SERIALS[1], 'draw', tool='finger', shift=120)
    wait_until(lambda: converged(1))
    same_image()
    report('PASS finger input is ignored by the pen tool')
    # Verify partial strokes reach the other running app before finger-up.
    for serial in SERIALS:
        adb(serial, 'shell', 'settings', 'put', 'system', 'screen_off_timeout', '2147483647')
        adb(serial, 'shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        adb(serial, 'shell', 'wm', 'dismiss-keyguard')
        adb(serial, 'shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
        def editor_ready():
            nodes=bridges[serial].ui_nodes()
            return any(n['content-desc']=='Document actions' for n in nodes) and any(n['text']=='infinite' for n in nodes)
        wait_until(editor_ready,30)
    wait_until(lambda: converged(1))
    baseline = {e['op'].get('stroke') for e in events('infinite') if e['op']['kind'] == 'stroke'}
    # Observe the live proxy without repeatedly starting an ART VM during the
    # timed gesture. Direct-file consistency is checked by converged() afterwards.
    observers=[]
    for serial in SERIALS:
        descriptor=json.loads(adb(serial,'shell','run-as','com.notemaker','cat','files/proxy.json'))
        forwarded=int(adb(serial,'forward','tcp:0','tcp:'+str(urllib.parse.urlparse(descriptor['url']).port)).strip())
        observers.append((forwarded,descriptor))
    def observe(command, index=1):
        forwarded,descriptor=observers[index]
        request=urllib.request.Request(f'http://127.0.0.1:{forwarded}/command',data=json.dumps({'document':'infinite','actor':'observer','args':[command]}).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+descriptor['token']})
        return json.load(urllib.request.urlopen(request,timeout=3))['result']
    # A visible Activity can precede its restarted proxy's upstream handshake.
    # Establish readiness before timing delivery, without extending the budget.
    wait_until(lambda: all(observe('status',i)['connected'] for i in range(2)),30)
    bridges[SERIALS[0]].close()
    swipe = subprocess.Popen(['adb', '-s', SERIALS[0], 'shell', 'am', 'instrument', '-w', '-e', 'action', 'live-draw', 'com.notemaker/.Integration'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def received_partial():
        if swipe.poll() is not None:
            output,error=swipe.communicate()
            raise AssertionError('Live-draw instrumentation ended before remote partial stroke arrived; stdout='+output+' stderr='+error)
        partial = [e['op'] for e in events('infinite') if e['op']['kind'] == 'stroke' and e['op'].get('stroke') not in baseline]
        return any(len(o['points']) >= 2 for o in partial)

    wait_until(received_partial, 16)
    output, error = swipe.communicate(timeout=15)
    assert swipe.returncode == 0
    assert 'PASS live-draw' in output, output + error
    for serial in SERIALS:
        adb(serial, 'shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
    wait_until(lambda: all(observe('status',i)['connected'] for i in range(2)),30)
    def live_converged():
        statuses=[observe('status',i) for i in range(2)]
        return all(status['pending']==0 for status in statuses) and statuses[0]['cursor']==statuses[1]['cursor'] and statuses[0]['cursor']>4
    wait_until(live_converged,60)
    same_image()
    adb(SERIALS[0], 'shell', 'am', 'start', '-f', '0x20000000', '-n', 'com.notemaker/.MainActivity')
    time.sleep(2)
    with (OUT / 'android-ui.png').open('wb') as screenshot:
        subprocess.check_call(['adb', '-s', SERIALS[0], 'exec-out', 'screencap', '-p'], stdout=screenshot)
    report('PASS live partial stylus stroke reaches the server before pen-up; both apps converge after completion')
    # Open an existing multipage document with the default fixed-page creation choice.
    pages_file = 'files/document-' + str(uuid.UUID(bytes=hashlib.md5(b'pages').digest(), version=3)) + '.json'
    bridges[SERIALS[1]].close();adb(SERIALS[1], 'shell', 'am', 'force-stop', 'com.notemaker')
    adb(SERIALS[1], 'shell', 'run-as', 'com.notemaker', 'rm', pages_file, pages_file+'.journal')
    instrument(SERIALS[1], 'configure', document='pages', host='http://10.0.2.2:18765', type='fixed')

    def existing_type_received():
        saved = read_replica(SERIALS[1], "pages")
        creations = [o for o in saved['events'] if o['kind'] == 'document']
        return not saved['pending'] and creations and creations[0]['type'] == 'pages'

    wait_until(existing_type_received)
    assert instrument(SERIALS[1], 'state')['type'] == 'pages'
    viewer = urllib.request.urlopen('http://127.0.0.1:18765/').read()
    assert b'NoteMaker live viewer' in viewer and b'canvas' in viewer
    report('PASS browser viewer served; existing document type retained')
    report('ALL INTEGRATION CHECKS PASSED')
    completed = True
finally:
    for bridge in bridges.values():
        bridge.close()
    if server is not None and server.poll() is None:
        server.terminate();server.wait(timeout=10)
    for serial, emulator in emulators.items():
        if not completed:
            try:
                pid = emulator.adb('shell', 'pidof', 'com.notemaker', timeout=10).strip()
                if pid:
                    emulator.save(OUT / (serial + '-failure-threads.txt'),
                                  'shell', 'run-as', 'com.notemaker', 'debuggerd', '-j', pid)
            except Exception:
                pass
            emulator.save(OUT / (serial + '-failure-logcat.txt'), 'logcat', '-d', '-s', 'NoteMaker', 'AndroidRuntime')
            emulator.save(OUT / (serial + '-failure.png'), 'exec-out', 'screencap', '-p', timeout=10, binary=True)
        emulator.stop()
    avd_root.cleanup()

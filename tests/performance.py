#!/usr/bin/env python3
"""1,000 real stylus strokes, frame timings, growth limits and durable restart on API 36."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.test-artifacts/performance'
OUT.mkdir(parents=True, exist_ok=True)
SERIAL = 'emulator-5586'

def adb(*args, timeout=60):
    return subprocess.check_output(['adb', '-s', SERIAL, *args], text=True, stderr=subprocess.STDOUT, timeout=timeout)

def check(metrics):
    assert metrics['strokes'] == 1000 and metrics['incremental_matches_replay']
    assert metrics['ink_cache_rebuilds'] == 0, metrics
    assert metrics['max_save_queue'] <= 64, metrics
    assert metrics['drain_ms'] < 5000, metrics
    first, _, last = metrics['windows']
    for window in metrics['windows']:
        for key, limit in [('handler', 16), ('dispatch', 50), ('input_to_draw', 100), ('frame', 100)]:
            sample = window[key]
            assert sample['samples'] >= 30, (window['strokes'], key, sample)
            assert sample['p95_ms'] < limit, (window['strokes'], key, sample, limit)
            assert sample['max_ms'] < 500, (window['strokes'], key, sample)
    for key, allowance in [('handler', 8), ('dispatch', 20), ('input_to_draw', 25), ('frame', 25)]:
        assert last[key]['p95_ms'] <= first[key]['p95_ms'] * 2 + allowance, (key, first[key], last[key])

assert SERIAL not in subprocess.check_output(['adb', 'devices'], text=True), 'Reserved emulator is already running'
with tempfile.TemporaryDirectory(prefix='notemaker-performance-') as temp:
    env = dict(os.environ, ANDROID_AVD_HOME=temp+'/avd')
    if Path('/run/opengl-driver/lib').exists():
        env['LD_LIBRARY_PATH']=os.environ.get('NOTEMAKER_GL_LIBRARY_PATH','')+':/run/opengl-driver/lib:'+env.get('LD_LIBRARY_PATH','')
    log = (OUT/'emulator.log').open('w')
    process = subprocess.Popen(['boox-emulator-headless', '-port', '5586', '-memory', '2048', '-cores', '4', '-gpu', os.environ.get('NOTEMAKER_TEST_GPU', 'host'), '-feature', '-Vulkan', '-wipe-data'], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            if process.poll() is not None: raise AssertionError('Emulator exited; see emulator.log')
            try:
                if adb('shell', 'getprop', 'sys.boot_completed', timeout=5).strip() == '1': break
            except Exception: pass
            time.sleep(1)
        else: raise AssertionError('Emulator boot timed out')
        time.sleep(20)  # Let first-boot package setup settle before collecting frame timings.
        adb('install', '-r', str(ROOT/'apps/android/build/notemaker.apk'))
        print('Running 1,000 strokes on isolated API 36 emulator; timings include real window frames.', flush=True)
        output = adb('shell', 'am', 'instrument', '-w', '-e', 'action', 'performance', 'com.notemaker/.Integration', timeout=600)
        (OUT/'instrumentation.txt').write_text(output)
        assert 'PASS performance' in output, output
        metrics = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/performance.json'))
        (OUT/'metrics.json').write_text(json.dumps(metrics, indent=2))
        print(json.dumps(metrics, indent=2), flush=True)
        check(metrics)
        adb('shell', 'am', 'force-stop', 'com.notemaker')
        apk = adb('shell', 'pm', 'path', 'com.notemaker').strip().removeprefix('package:')
        model = json.loads(adb('shell', 'run-as', 'com.notemaker', 'env', 'CLASSPATH='+apk, '/system/bin/app_process', '/system/bin', 'com.notemaker.cli.Main', '--state', '/data/user/0/com.notemaker/files', '--document', metrics['document'], 'inspect'))
        assert len(model['strokes']) == 1000
        assert all(len(stroke['points']) == 50 for stroke in model['strokes'])
        reopened=adb('shell','am','instrument','-w','-e','action','state','com.notemaker/.Integration',timeout=90)
        (OUT/'reopened.txt').write_text(reopened)
        assert 'PASS state' in reopened, reopened
        ui=json.loads(adb('shell','run-as','com.notemaker','cat','files/integration.json'))
        assert ui['strokes']==1000 and ui['model']==model
        print('PASS latency budgets, growth limits, incremental replay equivalence, 1,000 durable strokes after restart, and cached/uncached canvas comparison', flush=True)
    finally:
        try:
            (OUT/'logcat.txt').write_text(adb('logcat', '-d', '-v', 'threadtime'))
            adb('emu', 'kill')
        except Exception: pass
        try: process.wait(timeout=20)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
        log.close()

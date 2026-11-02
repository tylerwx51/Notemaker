#!/usr/bin/env python3
"""Strict <30ms input-to-draw, including 500-point crossings, growth and restart on API 36."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PORT = int(os.environ.get('NOTEMAKER_PERFORMANCE_PORT', '5586'))
SERIAL = os.environ.get('NOTEMAKER_PERFORMANCE_SERIAL', f'emulator-{PORT}')
OUT = ROOT / os.environ.get('NOTEMAKER_PERFORMANCE_OUT', '.test-artifacts/performance')
OUT.mkdir(parents=True, exist_ok=True)

def adb(*args, timeout=60):
    return subprocess.check_output(['adb', '-s', SERIAL, *args], text=True, stderr=subprocess.STDOUT, timeout=timeout)

def check(metrics):
    assert metrics['strokes'] == 1000 and metrics['incremental_matches_replay']
    tile_cache = metrics.get('tile_cache')
    assert isinstance(tile_cache, dict), metrics
    for key in ('tile_renders', 'item_renders', 'geometry_renders', 'path_command_reuses', 'path_command_misses',
                'current_bytes', 'peak_bytes', 'max_bytes', 'path_command_bytes_estimate',
                'path_command_points_estimate', 'max_path_command_bytes_estimate',
                'max_path_command_points_estimate'):
        assert isinstance(tile_cache.get(key), (int, float)) and tile_cache[key] >= 0, (key, tile_cache)
    assert tile_cache['tile_renders'] > 0 and tile_cache['reused_tile_repaints'] > 0 and tile_cache['item_renders'] > 0 and tile_cache['reused_tiles'] > 0, tile_cache
    assert tile_cache['geometry_renders'] > 0 and tile_cache['path_command_reuses'] > 0, tile_cache
    assert tile_cache['current_bytes'] <= tile_cache['max_bytes'], tile_cache
    assert tile_cache['peak_bytes'] <= tile_cache['max_bytes'], tile_cache
    assert tile_cache['path_command_bytes_estimate'] <= tile_cache['max_path_command_bytes_estimate'], tile_cache
    assert tile_cache['path_command_points_estimate'] <= tile_cache['max_path_command_points_estimate'], tile_cache
    assert metrics['max_save_queue'] <= 64, metrics
    assert metrics['drain_ms'] < 5000, metrics
    # Gate the worst event, not just a percentile: a stalled event must fail.
    # Every measured input must reach a draw, including coalesced moves and lift.
    for key, count in [('stroke_latency', 990 * 8), ('down_latency', 990),
                       ('move_latency', 990 * 6), ('up_latency', 990), ('figure_eight_latency', 10 * 8)]:
        sample = metrics[key]
        assert sample['samples'] == count, (key, sample, count)
        assert sample['max_ms'] < 30 and sample['over_30_ms'] == 0, (key, sample, 'strict <30ms budget')
    first, _, last = metrics['windows']
    for window in metrics['windows']:
        for key, limit in [('handler', 16), ('dispatch', 50), ('input_to_draw', 100), ('frame', 100)]:
            sample = window[key]
            assert sample['samples'] >= 30, (window['strokes'], key, sample)
            assert sample['p95_ms'] < limit, (window['strokes'], key, sample, limit)
            assert sample['max_ms'] < 500, (window['strokes'], key, sample)
    for key, allowance in [('handler', 8), ('dispatch', 20), ('input_to_draw', 25), ('frame', 25)]:
        assert last[key]['p95_ms'] <= first[key]['p95_ms'] * 2 + allowance, (key, first[key], last[key])

def wait_for_reserved_emulator_release(seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        devices = subprocess.check_output(['adb', 'devices'], text=True)
        if SERIAL not in devices:
            return
        time.sleep(.5)
    raise AssertionError(f'Reserved emulator {SERIAL} is still registered after {seconds}s')

wait_for_reserved_emulator_release()
with tempfile.TemporaryDirectory(prefix='notemaker-performance-') as temp:
    env = dict(os.environ, ANDROID_AVD_HOME=temp+'/avd')
    if Path('/run/opengl-driver/lib').exists():
        env['LD_LIBRARY_PATH']=os.environ.get('NOTEMAKER_GL_LIBRARY_PATH','')+':/run/opengl-driver/lib:'+env.get('LD_LIBRARY_PATH','')
    log = (OUT/'emulator.log').open('w')
    process = subprocess.Popen(['boox-emulator-headless', '-port', str(PORT), '-memory', '2048', '-cores', '4', '-gpu', os.environ.get('NOTEMAKER_TEST_GPU', 'host'), '-feature', '-Vulkan', '-wipe-data'], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            if process.poll() is not None: raise AssertionError('Emulator exited; see emulator.log')
            try:
                if adb('shell', 'getprop', 'sys.boot_completed', timeout=5).strip() == '1': break
            except Exception: pass
            time.sleep(1)
        else: raise AssertionError('Emulator boot timed out')
        # This benchmark exercises offline ink. Keep first-boot Google/WebView
        # downloads from competing with the measured app on the disposable AVD.
        adb('shell', 'svc', 'wifi', 'disable')
        adb('shell', 'svc', 'data', 'disable')
        # force-stop is insufficient: Android restarts these unrelated first-boot
        # network jobs. NoteMaker's offline ink has no Google services dependency.
        for package in ('com.google.android.googlequicksearchbox', 'com.google.android.gms'):
            adb('shell', 'pm', 'disable-user', '--user', '0', package)
            assert 'package:' + package in adb('shell', 'pm', 'list', 'packages', '-d').splitlines()
        time.sleep(3)  # Disabled first-boot packages have time to leave the frame scheduler.
        adb('install', '-r', str(ROOT/'apps/android/build/notemaker.apk'))
        print('Running 1,000 strokes on isolated API 36 emulator; timings include real window frames.', flush=True)
        output = adb('shell', 'am', 'instrument', '-w', '-e', 'action', 'performance', 'com.notemaker/.Integration', timeout=600)
        (OUT/'instrumentation.txt').write_text(output)
        assert 'PASS performance' in output, output
        metrics = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/performance.json'))
        (OUT/'metrics.json').write_text(json.dumps(metrics, indent=2))
        resources = json.loads(adb('shell', 'run-as', 'com.notemaker', 'cat', 'files/performance-resources.json'))
        (OUT/'resources.json').write_text(json.dumps(resources, indent=2))
        assert [sample['strokes'] for sample in resources['samples']] == list(range(0, 1001, 100))
        assert all(sample['pss_kib'] > 0 for sample in resources['samples'])
        print(json.dumps(metrics, indent=2), flush=True)
        check(metrics)
        adb('shell', 'am', 'force-stop', 'com.notemaker')
        from replica import cli_command
        model = json.loads(subprocess.check_output(cli_command(SERIAL) + ['--state', '/data/user/0/com.notemaker/files', '--document', metrics['document'], 'inspect'], text=True, timeout=60))
        assert len(model['strokes']) == 1000
        sizes = [len(stroke['points']) for stroke in model['strokes']]
        assert sizes.count(50) == 990 and sizes.count(500) == 10, sizes
        reopened=adb('shell','am','instrument','-w','-e','action','state','-e','verifyCache','true','com.notemaker/.Integration',timeout=90)
        (OUT/'reopened.txt').write_text(reopened)
        if 'PASS state' not in reopened:
            for name in ('cache-diff-cached.png', 'cache-diff-reference.png', 'cache-diff.json'):
                try:
                    data = subprocess.check_output(['adb', '-s', SERIAL, 'exec-out', 'run-as', 'com.notemaker', 'cat', f'files/{name}'], timeout=20)
                    (OUT/name).write_bytes(data)
                except subprocess.CalledProcessError:
                    pass
        assert 'PASS state' in reopened, reopened
        ui=json.loads(adb('shell','run-as','com.notemaker','cat','files/integration.json'))
        assert ui['strokes']==1000 and ui['model']==model
        print('PASS strict <30ms for every measured input, including 500-point figure eights; growth limits, replay equivalence, and 1,000 durable strokes after restart', flush=True)
    finally:
        try:
            (OUT/'logcat.txt').write_text(adb('logcat', '-d', '-v', 'threadtime'))
            adb('emu', 'kill')
        except Exception: pass
        try: process.wait(timeout=20)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
        log.close()

#!/usr/bin/env python3
"""File browsing and creation on an isolated offline Android emulator."""
import os
import subprocess
import tempfile
import time
import drawing_android as ui


def browse():
    ui.tap('Document actions')
    ui.tap('Browse files…')
    time.sleep(.5)


def main():
    deadline=time.monotonic()+30
    while ui.SERIAL in ui.run('adb', 'devices') and time.monotonic()<deadline:
        time.sleep(.5)
    assert ui.SERIAL not in ui.run('adb', 'devices'), f'Reserved emulator {ui.SERIAL} is still running'
    with tempfile.TemporaryDirectory(prefix='notemaker-files-') as temp:
        with (ui.OUT / 'files-emulator.log').open('w') as log:
            emulator = subprocess.Popen(
                ['boox-emulator-headless', '-port', '5590', '-memory', '1536', '-cores', '2', '-wipe-data'],
                env=dict(os.environ, ANDROID_AVD_HOME=temp + '/avd'), stdout=log, stderr=subprocess.STDOUT)
            try:
                ui.wait_ready()
                ui.adb('install', '-r', str(ui.APK))
                ui.instrument('configure', document='Original', host='offline')
                ui.instrument('draw')
                ui.instrument('file-preview')
                time.sleep(1)
                ui.adb('shell', 'am', 'force-stop', 'com.notemaker')
                ui.adb('shell', 'am', 'start', '-W', '-n', 'com.notemaker/.MainActivity')
                browse()
                assert 'Files grid' in ui.ui_dump()
                for _ in range(10):
                    if 'Preview of Original' in ui.ui_dump():
                        break
                    time.sleep(.5)
                else:
                    raise AssertionError('Grid preview did not load')
                ui.screenshot('files-grid')
                ui.tap('List')
                assert 'Files list' in ui.ui_dump()
                ui.screenshot('files-list')
                ui.tap('New file')
                ui.tap('Create')
                assert 'Enter a file name' in ui.ui_dump()
                ui.tap('File name')
                ui.adb('shell', 'input', 'text', 'Browser-test')
                ui.tap('Fixed page')
                ui.tap('Multiple pages')
                ui.tap('Create')
                time.sleep(1)
                assert 'Browser-test' in ui.adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
                browse()
                ui.tap('New file')
                ui.tap('File name')
                ui.adb('shell', 'input', 'text', 'Browser-test')
                ui.tap('Create')
                assert 'Document already exists' in ui.ui_dump()
                ui.tap('Cancel')
                ui.tap('Search files')
                ui.adb('shell', 'input', 'text', 'missing')
                assert 'No matching files' in ui.ui_dump()
                ui.adb('shell', 'input', 'keyevent', '4')
                ui.tap('Close')
                browse()
                ui.tap('Original')
                assert 'Original' in ui.adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
                ui.adb('shell', 'am', 'force-stop', 'com.notemaker')
                ui.adb('shell', 'am', 'start', '-n', 'com.notemaker/.MainActivity')
                browse()
                assert 'Files list' in ui.ui_dump()
                ui.tap('Grid')
                assert 'Files grid' in ui.ui_dump()
                ui.screenshot('files-browser')
                ui.tap('Browser-test')
                assert ui.latest_state()['type'] == 'pages'
                print('PASS grid previews, list mode persistence, offline creation, validation, search, selection, restart and retained type')
            finally:
                try:
                    (ui.OUT / 'files-logcat.txt').write_text(ui.adb('logcat', '-d', '-t', '2000'))
                except Exception:
                    pass
                try:
                    ui.adb('emu', 'kill')
                except Exception:
                    pass
                emulator.terminate()
                try:
                    emulator.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    emulator.kill()
                    emulator.wait()


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""File browsing and creation on an isolated offline Android emulator."""
import os
import re
import xml.etree.ElementTree as ET
import tempfile
import time
os.environ.setdefault('NOTEMAKER_DRAWING_ANDROID_PORT', '5594')
os.environ.setdefault('NOTEMAKER_DRAWING_ANDROID_OUT', '.test-artifacts/files-android')
import drawing_android as ui
from android_emulator import Emulator


def browse():
    ui.tap('Document actions')
    ui.tap('Browse files…')
    time.sleep(.5)


def hold_file(name):
    node = next(n for n in ET.fromstring(ui.ui_dump()).iter('node') if n.attrib.get('text') == name)
    left, top, right, bottom = map(int, re.findall(r'\d+', node.attrib['bounds']))
    x, y = str((left+right)//2), str((top+bottom)//2)
    ui.adb('shell','input','swipe',x,y,x,y,'1000')
    time.sleep(.5)


def await_ui(text, present=True, seconds=10):
    deadline=time.monotonic()+seconds
    hierarchy=''
    while time.monotonic()<deadline:
        hierarchy=ui.ui_dump()
        if (text in hierarchy)==present:return hierarchy
        time.sleep(.15)
    raise AssertionError(f'Expected {text!r} present={present} in Android UI; last hierarchy: {hierarchy}')


def main():
    deadline=time.monotonic()+30
    while ui.SERIAL in ui.run('adb', 'devices') and time.monotonic()<deadline:
        time.sleep(.5)
    with tempfile.TemporaryDirectory(prefix='notemaker-files-') as temp:
        emulator = Emulator(ui.PORT, ui.OUT / 'files-emulator.log', temp + '/avd')
        try:
            emulator.start().wait_booted()
            emulator.install(ui.APK)
            ui.instrument('configure', document='Original', host='offline')
            ui.instrument('draw')
            ui.instrument('file-preview')
            time.sleep(1)
            ui.bridge.close();ui.adb('shell', 'am', 'force-stop', 'com.notemaker')
            ui.adb('shell', 'am', 'start', '-W', '-n', 'com.notemaker/.MainActivity')
            browse()
            await_ui('Files grid')
            await_ui('Preview of Original')
            ui.screenshot('files-grid')
            ui.tap('List')
            await_ui('Files list')
            ui.screenshot('files-list')
            ui.tap('New file')
            ui.tap('Create')
            await_ui('Enter a file name')
            ui.tap('File name')
            emulator.type_text('Browser-test')
            ui.tap('Fixed page')
            # The dropdown reaches the accessibility tree late under load; a
            # second tap while it is opening would close it again. Tap again
            # only when the first tap evidently did not open it.
            try:
                await_ui('Multiple pages', seconds=5)
            except AssertionError:
                ui.tap('Fixed page')
                await_ui('Multiple pages', seconds=10)
            ui.tap('Multiple pages')
            ui.tap('Create')
            time.sleep(1)
            assert 'Browser-test' in ui.adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
            browse()
            ui.tap('New file')
            ui.tap('File name')
            emulator.type_text('Browser-test')
            ui.tap('Create')
            await_ui('Document already exists')
            ui.tap('Cancel')
            if 'Search files' not in ui.ui_dump():
                browse()
            ui.tap('Search files')
            emulator.type_text('missing')
            await_ui('No matching files')
            ui.adb('shell', 'input', 'keyevent', '4')
            ui.tap('Close')
            browse()
            ui.tap('Original')
            assert 'Original' in ui.adb('shell', 'run-as', 'com.notemaker', 'cat', 'shared_prefs/settings.xml')
            ui.bridge.close();ui.adb('shell', 'am', 'force-stop', 'com.notemaker')
            ui.adb('shell', 'am', 'start', '-n', 'com.notemaker/.MainActivity')
            browse()
            await_ui('Files list')
            ui.tap('Grid')
            await_ui('Files grid')
            ui.screenshot('files-browser')
            ui.tap('Browser-test')
            assert ui.latest_state()['type'] == 'pages'
            browse()
            hold_file('Browser-test')
            ui.tap('Cancel')
            await_ui('Browser-test')
            hold_file('Browser-test')
            # Exact button text avoids matching the confirmation title.
            node=next(n for n in ET.fromstring(ui.ui_dump()).iter('node') if n.attrib.get('text','').casefold() == 'delete')
            left,top,right,bottom=map(int,re.findall(r'\d+',node.attrib['bounds']))
            ui.adb('shell','input','tap',str((left+right)//2),str((top+bottom)//2))
            time.sleep(1)
            await_ui('Browser-test',present=False)
            ui.tap('Close')
            await_ui('Deleted')
            ui.screenshot('files-deleted')
            ui.bridge.close();ui.adb('shell','am','force-stop','com.notemaker')
            ui.adb('shell','am','start','-W','-n','com.notemaker/.MainActivity')
            browse()
            await_ui('Browser-test',present=False)
            ui.tap('Original')
            assert not ui.latest_state()['model']['deleted']
            print('PASS file browser, offline creation, deletion confirmation/cancel, active-file deletion and restart persistence')
        finally:
            ui.bridge.close()
            emulator.save(ui.OUT / 'files-logcat.txt', 'logcat', '-d', '-t', '2000')
            emulator.stop()


if __name__ == '__main__':
    main()

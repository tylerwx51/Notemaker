#!/usr/bin/env python3
"""Exercise live UI session control through real CLI calls and the tablet proxy."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
SERIAL="emulator-5592"
CLI=ROOT/"apps/cli/notemaker"
APK=ROOT/"apps/android/build/notemaker.apk"

def run(*args,timeout=45):
    return subprocess.check_output(args,text=True,stderr=subprocess.STDOUT,timeout=timeout).strip()

def adb(*args):return run("adb","-s",SERIAL,*args)

def wait(fn,seconds=120):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            result=fn()
            if result:return result
        except (subprocess.CalledProcessError, ValueError):pass
        time.sleep(.5)
    raise AssertionError("Timed out waiting for Android session")

def main():
    assert SERIAL not in run("adb","devices"),"Reserved emulator is already running"
    with tempfile.TemporaryDirectory(prefix="notemaker-ui-session-") as root:
        log=open(Path(root)/"emulator.log","w")
        emulator=subprocess.Popen(["boox-emulator-headless","-port","5592","-memory","1536","-cores","2","-wipe-data"],env=dict(os.environ,ANDROID_AVD_HOME=str(Path(root)/"avd")),stdout=log,stderr=subprocess.STDOUT)
        try:
            wait(lambda:adb("shell","getprop","sys.boot_completed")=="1",240)
            adb("install","-r",str(APK))
            adb("shell","am","start","-W","-n","com.notemaker/.MainActivity")
            descriptor=wait(lambda:json.loads(adb("shell","run-as","com.notemaker","cat","files/proxy.json")))
            adb("forward","tcp:18792","tcp:8766")
            descriptor["url"]="http://127.0.0.1:18792"
            proxy=Path(root)/"proxy.json";proxy.write_text(json.dumps(descriptor))
            def cli(*args,value=None,document="notes"):
                p=subprocess.run([str(CLI),"--proxy-file",str(proxy),"--document",document,*args],input=json.dumps(value) if value is not None else None,text=True,capture_output=True,timeout=45)
                assert p.returncode==0,(p.stdout,p.stderr)
                return json.loads(p.stdout)
            first=wait(lambda:cli("ui","sessions")["sessions"])
            session=first[0]["session"]
            assert cli("ui","state",session)["document"]=="notes"
            catalog=cli("ui","actions",session)["actions"]
            assert "files.open" in catalog and "page.open" in catalog
            assert cli("ui","action",session,"files.open","-",value={})["state"]["filesOpen"]
            assert cli("ui","action",session,"files.mode","-",value={"mode":"list"})["state"]["files"]["mode"]=="list"
            assert cli("ui","action",session,"files.search","-",value={"query":"notes"})["state"]["files"]["query"]=="notes"
            assert not cli("ui","action",session,"files.close","-",value={})["state"]["filesOpen"]
            cli("document","create","pages",document="session-pages")
            opened=cli("ui","action",session,"document.open","-",value={"name":"session-pages"})["state"]
            assert opened["document"]=="session-pages" and opened["documentType"]=="pages",opened
            assert cli("ui","action",session,"page.open","-",value={"page":3})["state"]["page"]==3
            assert cli("ui","action",session,"page.previous","-",value={})["state"]["page"]==2
            assert cli("ui","action",session,"tool.select","-",value={"name":"eraser"})["state"]["tool"]=="eraser"
            assert cli("ui","action",session,"pen.configure","-",value={"brush":"pencil","width":7})["state"]["width"]==7
            cli("ui","action",session,"pen.preset.save","-",value={"name":"CLI pencil"})
            cli("ui","action",session,"pen.configure","-",value={"brush":"pen","width":2})
            preset=cli("ui","action",session,"pen.preset.apply","-",value={"name":"CLI pencil"})["state"]
            assert preset["brush"]=="pencil" and preset["width"]==7,preset
            assert cli("ui","action",session,"eraser.configure","-",value={"mode":"partial","radius":18})["state"]["eraserRadius"]==18
            assert cli("ui","action",session,"view.zoom","-",value={"factor":1.25})["state"]["zoom"]>1
            assert cli("ui","action",session,"view.reset","-",value={})["state"]["zoom"]==1
            adb("shell","input","keyevent","KEYCODE_HOME")
            assert cli("ui","state",session)["session"]==session
            adb("shell","am","start","-W","-n","com.notemaker/.MainActivity")
            assert cli("ui","state",session)["session"]==session
            adb("shell","am","force-stop","com.notemaker")
            adb("shell","am","start","-W","-n","com.notemaker/.MainActivity")
            second=wait(lambda:cli("ui","sessions")["sessions"])
            assert second[0]["session"]!=session,(first,second)
            print("PASS live session discovery, file navigation, document/page/tool controls, and new ID after restart")
        finally:
            try:adb("emu","kill")
            except Exception:pass
            emulator.terminate()
            try:emulator.wait(timeout=15)
            except subprocess.TimeoutExpired:emulator.kill();emulator.wait()
            log.close()

if __name__=="__main__":main()

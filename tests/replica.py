"""Read a live Android replica through its CLI instead of private storage formats."""
import json
import subprocess

def read_replica(serial, document):
    apk = subprocess.check_output(["adb", "-s", serial, "shell", "pm", "path", "com.notemaker"], text=True).strip().removeprefix("package:")
    output = subprocess.check_output(["adb", "-s", serial, "shell", "run-as", "com.notemaker", "env", "CLASSPATH="+apk,
        "/system/bin/app_process", "/system/bin", "com.notemaker.cli.Main", "--state", "/data/user/0/com.notemaker/files", "--document", document, "log"], text=True, timeout=30)
    return json.loads(output)

"""Read a live Android replica through its CLI instead of private storage formats."""
import json
import subprocess

ABI_DIRS = {"arm64-v8a": "arm64", "x86_64": "x86_64"}


def cli_command(serial, package="com.notemaker"):
    """Command prefix running the APK's packaged native CLI under the app UID."""
    apk = subprocess.check_output(["adb", "-s", serial, "shell", "pm", "path", package], text=True).strip().splitlines()[0].removeprefix("package:")
    abi = subprocess.check_output(["adb", "-s", serial, "shell", "getprop", "ro.product.cpu.abi"], text=True).strip()
    library = apk.rsplit("/", 1)[0] + "/lib/" + ABI_DIRS.get(abi, abi) + "/libnotemaker_cli.so"
    return ["adb", "-s", serial, "shell", "run-as", package, library]


def read_replica(serial, document):
    output = subprocess.check_output(cli_command(serial) + ["--state", "/data/user/0/com.notemaker/files", "--document", document, "log"], text=True, timeout=30)
    return json.loads(output)

"""Disposable API 36 test emulators with bounded boot, install and cleanup.

Every ADB call here has a timeout: `adb -s SERIAL logcat` (and other commands)
wait indefinitely for a serial that never registered, which is exactly the
state an emulator that failed to boot leaves behind.
"""
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
APK = ROOT / 'apps/android/build/notemaker.apk'
BOOT_SECONDS = float(os.environ.get('NOTEMAKER_TEST_BOOT_SECONDS', '300'))


class Emulator:
    def __init__(self, port, log_path, avd_home, memory=1536, cores=2, env=None):
        self.port = int(port)
        self.serial = f'emulator-{self.port}'
        self.log_path = Path(log_path)
        self.avd_home = Path(avd_home)
        self.memory = memory
        self.cores = cores
        self.env = env
        self.process = None
        self.log = None

    def adb(self, *args, timeout=60):
        return subprocess.check_output(['adb', '-s', self.serial, *args], text=True,
                                       stderr=subprocess.STDOUT, timeout=timeout)

    def start(self):
        devices = subprocess.check_output(['adb', 'devices'], text=True, timeout=30)
        assert self.serial not in devices, f'Reserved emulator {self.serial} is already running'
        self.log = self.log_path.open('w')
        env = dict(self.env if self.env is not None else os.environ, ANDROID_AVD_HOME=str(self.avd_home))
        self.process = subprocess.Popen(
            ['boox-emulator-headless', '-port', str(self.port), '-memory', str(self.memory),
             '-cores', str(self.cores), '-gpu', os.environ.get('NOTEMAKER_TEST_GPU', 'host'),
             '-feature', '-Vulkan', '-wipe-data'],
            env=env, stdout=self.log, stderr=subprocess.STDOUT)
        return self

    def wait_booted(self, seconds=None):
        """Wait for boot completion, unlocked user storage and the package manager."""
        seconds = BOOT_SECONDS if seconds is None else seconds
        deadline = time.monotonic() + seconds
        checks = [
            (('shell', 'getprop', 'sys.boot_completed'), lambda out: out.strip() == '1'),
            (('shell', 'am', 'get-started-user-state', '0'), lambda out: 'RUNNING_UNLOCKED' in out),
            (('shell', 'pm', 'path', 'android'), lambda out: out.startswith('package:')),
        ]
        last = 'no response'
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f'{self.serial} exited during boot (code {self.process.returncode}); '
                                   f'see {self.log_path}')
            try:
                for args, ready in checks:
                    output = self.adb(*args, timeout=10)
                    if not ready(output):
                        last = f'{" ".join(args[1:])}: {output.strip()!r}'
                        break
                else:
                    return
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                last = str(error.output or error).strip()
            time.sleep(1)
        raise TimeoutError(f'{self.serial} did not finish booting in {seconds:.0f}s ({last}); see {self.log_path}')

    def disable_animations(self):
        # Taps must not land on animating dialogs.
        for setting in ('window_animation_scale', 'transition_animation_scale', 'animator_duration_scale'):
            self.adb('shell', 'settings', 'put', 'global', setting, '0')

    def type_text(self, text, seconds=15):
        """Type into the field just tapped, once the IME has bound to it.

        Key events injected while the IME is still starting on a newly focused
        field can be applied twice (for example `TToolbar-text`)."""
        deadline = time.monotonic() + seconds
        while 'mInputShown=true' not in self.adb('shell', 'dumpsys', 'input_method', timeout=15):
            if time.monotonic() > deadline:
                raise TimeoutError(f'The input method did not open on {self.serial} before typing {text!r}')
            time.sleep(.2)
        self.adb('shell', 'input', 'text', text)

    def install(self, apk=APK):
        # A full transfer: API 30+ otherwise installs incrementally whenever
        # the v4 signature (.idsig) exists, and lazily streamed native
        # libraries can still be empty when the app first loads them.
        output = self.adb('install', '--no-incremental', '-r', str(apk), timeout=300)
        assert 'Success' in output, output

    def save(self, path, *args, timeout=30, binary=False):
        """Best-effort diagnostic capture; never blocks cleanup for long."""
        try:
            devices = subprocess.check_output(['adb', 'devices'], text=True, timeout=10)
            if f'{self.serial}\tdevice' not in devices:
                return False  # ADB would wait for the device instead of failing.
            data = subprocess.run(['adb', '-s', self.serial, *args], capture_output=True,
                                  timeout=timeout, check=True).stdout
            Path(path).write_bytes(data) if binary else Path(path).write_text(data.decode(errors='replace'))
            return True
        except (OSError, subprocess.SubprocessError):
            return False

    def stop(self):
        """Always ends the emulator, even if it never registered with ADB.

        QEMU can ignore SIGTERM while its renderer is failing, so this
        escalates to SIGKILL. Only QEMU itself is signalled: the first
        emulator spawns the per-user netsimd that every running emulator
        shares, and killing its process group would break the others."""
        process, self.process = self.process, None
        if process is None:
            return
        try:
            if process.poll() is None:
                try:
                    self.adb('emu', 'kill', timeout=10)
                    process.wait(timeout=30)
                except (OSError, subprocess.SubprocessError):
                    pass
            for sig, seconds in ((signal.SIGTERM, 15), (signal.SIGKILL, 15)):
                if process.poll() is not None:
                    break
                process.send_signal(sig)
                try:
                    process.wait(timeout=seconds)
                except subprocess.TimeoutExpired:
                    pass
        finally:
            if self.log is not None:
                self.log.close()
                self.log = None
            # ADB can retain a disconnected emulator briefly after QEMU exits;
            # wait before releasing this suite's port to the next test.
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                try:
                    if self.serial not in subprocess.check_output(['adb', 'devices'], text=True, timeout=10):
                        break
                except (OSError, subprocess.SubprocessError):
                    pass
                time.sleep(.2)

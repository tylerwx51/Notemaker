#!/usr/bin/env python3
"""Profile a captured note on a disposable replica, without changing the installed app.

Run in nix-shell. Capture files with adb exec-out run-as com.notemaker tar -cf - files.
The archive and output contain private document data; keep them under .test-artifacts.
The probes are the Rust binaries built by apps/cli/build.sh (--android for devices).
Their heap_bytes/native_bytes are allocator in-use bytes (mallinfo), not JVM/ART heap
sizes, so they are not comparable with captures made by the former Java probes.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--document', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--serial', help='Measure Android on this device; otherwise the host')
    parser.add_argument('--android-bin', type=Path, help='Candidate probe binary; default: apps/cli/build.sh --android output')
    parser.add_argument('--probe', choices=['LatencyProbe','CacheFootprintProbe','CoverageParity'], default='LatencyProbe')
    args = parser.parse_args()
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False, mode=0o700)
    capture, replica = out/'capture', out/'replica'
    replica.mkdir()
    with tarfile.open(args.archive) as archive:
        archive.extractall(capture, filter='data')
    matched = False
    for file in (capture/'files').glob('document-*.json'):
        if json.loads(file.read_text()).get('document') == args.document:
            shutil.copy(file, replica/file.name)
            journal = Path(str(file)+'.journal')
            if journal.exists():
                shutil.copy(journal, replica/journal.name)
            matched = True
    if not matched:
        raise ValueError('Document absent from archive')
    (replica/'LATENCY-SCRATCH').touch()
    binary = {'LatencyProbe': 'latency_probe', 'CacheFootprintProbe': 'cache_footprint_probe', 'CoverageParity': 'coverage_parity'}[args.probe]
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT/'target'))
    if not args.serial:
        with (out/'timings.jsonl').open('w') as log:
            subprocess.run([str(target/'release'/binary), str(replica), args.document], stdout=log, check=True, timeout=600)
        return
    adb = ['adb', '-s', args.serial]
    abi = subprocess.check_output(adb+['shell', 'getprop', 'ro.product.cpu.abi'], text=True).strip()
    probe = args.android_bin or target/{'arm64-v8a': 'aarch64-linux-android', 'x86_64': 'x86_64-linux-android'}[abi]/'release'/binary
    remote = '/data/local/tmp/notemaker-latency-'+uuid.uuid4().hex
    subprocess.run(adb+['shell', 'mkdir', remote], check=True)
    try:
        subprocess.run(adb+['push', str(replica), remote+'/replica'], check=True)
        subprocess.run(adb+['push', str(probe), remote+'/probe'], check=True)
        subprocess.run(adb+['shell', 'chmod', '755', remote+'/probe'], check=True)
        command = [remote+'/probe', remote+'/replica', args.document]
        with (out/'timings.jsonl').open('w') as log:
            subprocess.run(adb+['shell', shlex.join(command)], stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
    finally:
        subprocess.run(adb+['shell', 'rm', '-rf', remote], check=True)
    print(out/'timings.jsonl')


if __name__ == '__main__':
    main()

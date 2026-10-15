#!/usr/bin/env python3
"""Profile a captured note on a disposable replica, without changing the installed app.

Run in nix-shell. Capture files with adb exec-out run-as com.notemaker tar -cf - files.
The archive and output contain private document data; keep them under .test-artifacts.
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
    parser.add_argument('--serial', help='Measure Android on this device; otherwise JVM')
    parser.add_argument('--android-jar', type=Path, help='Candidate CLI dex jar; default: installed APK')
    parser.add_argument('--probe', choices=['LatencyProbe','CacheFootprintProbe','CoverageParity'], default='LatencyProbe')
    args = parser.parse_args()
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False, mode=0o700)
    capture, replica, classes = out/'capture', out/'replica', out/'classes'
    replica.mkdir(); classes.mkdir()
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
    jar = ROOT/'apps/cli/build/notemaker.jar'
    sources=[ROOT/'tests/LatencyProbe.java']
    if args.probe!='LatencyProbe': sources.append(ROOT/('tests/'+args.probe+'.java'))
    subprocess.run(['javac', '--release', '8', '-cp', str(jar), '-d', str(classes), *map(str,sources)], check=True)
    if not args.serial:
        with (out/'timings.jsonl').open('w') as log:
            subprocess.run(['java', '-cp', str(classes)+':'+str(jar), args.probe, str(replica), args.document], stdout=log, check=True, timeout=600)
        return
    adb = ['adb', '-s', args.serial]
    dex = out/'dex'; dex.mkdir()
    sdk = Path(os.environ['ANDROID_HOME'])
    subprocess.run([str(sdk/'build-tools/36.0.0/d8'), '--min-api', '26', '--lib', str(sdk/'platforms/android-36/android.jar'), '--classpath', str(jar), '--output', str(dex), *map(str, classes.glob('*.class'))], check=True)
    probe = out/'probe.jar'
    subprocess.run(['jar', 'cf', str(probe), '-C', str(dex), 'classes.dex'], check=True)
    remote = '/data/local/tmp/notemaker-latency-'+uuid.uuid4().hex
    subprocess.run(adb+['shell', 'mkdir', remote], check=True)
    try:
        subprocess.run(adb+['push', str(replica), remote+'/replica'], check=True)
        subprocess.run(adb+['push', str(probe), remote+'/probe.jar'], check=True)
        if args.android_jar:
            apk = remote+'/candidate.jar'
            subprocess.run(adb+['push', str(args.android_jar), apk], check=True)
        else:
            apk = subprocess.check_output(adb+['shell', 'pm', 'path', 'com.notemaker'], text=True).strip().removeprefix('package:')
        command = ['env', 'CLASSPATH='+remote+'/probe.jar:'+apk, 'app_process', '/system/bin', args.probe, remote+'/replica', args.document]
        with (out/'timings.jsonl').open('w') as log:
            subprocess.run(adb+['shell', shlex.join(command)], stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
    finally:
        subprocess.run(adb+['shell', 'rm', '-rf', remote], check=True)
    print(out/'timings.jsonl')


if __name__ == '__main__':
    main()

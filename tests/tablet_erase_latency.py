#!/usr/bin/env python3
"""Measure real eraser input to updated Canvas draw in an isolated Android package."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import uuid
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[1]
PACKAGE='com.notemaker.latency'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial',required=True)
    parser.add_argument('--archive',type=Path,required=True)
    parser.add_argument('--document',required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--compile-speed',action='store_true',help='Measure explicit Android ahead-of-time compilation separately')
    parser.add_argument('--mode',choices=['partial','item'],default='partial')
    args=parser.parse_args()
    if not os.environ.get('IN_NIX_SHELL'): parser.error('Run through nix-shell')
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False,mode=0o700)
    adb=['adb','-s',args.serial]
    def call(*cmd,**kw): return subprocess.check_output(adb+list(cmd),timeout=kw.get('timeout',60))
    if ('package:'+PACKAGE) in call('shell','pm','list','packages',PACKAGE).decode().splitlines(): raise RuntimeError('Isolated package already exists')
    sdk=Path(os.environ['ANDROID_HOME']);build=sdk/'build-tools/36.0.0';platform=sdk/'platforms/android-36/android.jar'
    classes=out/'classes';shutil.copytree(ROOT/'apps/android/build/classes',classes)
    source=(ROOT/'apps/android/src/com/notemaker/ReplicaService.java').read_text()
    assert 'new LocalProxy(context.getFilesDir(),8766)' in source
    (out/'ReplicaService.java').write_text(source.replace('new LocalProxy(context.getFilesDir(),8766)','new LocalProxy(context.getFilesDir(),0)'))
    subprocess.run(['javac','-source','8','-target','8','-cp',str(platform)+':'+str(classes),'-d',str(classes),str(out/'ReplicaService.java')],check=True)
    manifest=ET.parse(ROOT/'apps/android/AndroidManifest.xml');root=manifest.getroot();root.set('package',PACKAGE)
    ns='{http://schemas.android.com/apk/res/android}'
    for element in root.iter():
        name=element.get(ns+'name','')
        if name.startswith('.'): element.set(ns+'name','com.notemaker'+name)
    root.find('instrumentation').set(ns+'targetPackage',PACKAGE)
    root.find('application').set(ns+'label','NoteMaker latency test')
    manifest.write(out/'AndroidManifest.xml',encoding='utf-8')
    subprocess.run([str(build/'aapt2'),'link','-o',str(out/'base.apk'),'--manifest',str(out/'AndroidManifest.xml'),'-I',str(platform)],check=True)
    dex=out/'dex';dex.mkdir()
    listing=out/'classes.list';listing.write_text('\n'.join(map(str,classes.rglob('*.class'))))
    subprocess.run([str(build/'d8'),'--min-api','26','--lib',str(platform),'--output',str(dex),'@'+str(listing),str(ROOT/'apps/android/build/androidsvg.jar')],check=True)
    subprocess.run(['zip','-q',str(out/'base.apk'),'classes.dex'],cwd=dex,check=True)
    apk=out/'latency.apk'
    subprocess.run([str(build/'zipalign'),'-f','4',str(out/'base.apk'),str(apk)],check=True)
    subprocess.run([str(build/'apksigner'),'sign','--ks',str(ROOT/'apps/android/build/debug.keystore'),'--ks-pass','pass:android',str(apk)],check=True)
    capture=out/'capture'
    with tarfile.open(args.archive) as archive: archive.extractall(capture,filter='data')
    replica=out/'replica';(replica/'files').mkdir(parents=True);(replica/'shared_prefs').mkdir()
    found=False
    for file in (capture/'files').glob('document-*.json'):
        if json.loads(file.read_text()).get('document')==args.document:
            shutil.copy(file,replica/'files'/file.name)
            journal=Path(str(file)+'.journal')
            if journal.exists(): shutil.copy(journal,replica/'files'/journal.name)
            found=True
    assert found,'Document absent'
    prefs=ET.Element('map')
    for key,value in [('document',args.document),('host','')]: ET.SubElement(prefs,'string',name=key).text=value
    ET.ElementTree(prefs).write(replica/'shared_prefs/settings.xml',encoding='utf-8')
    remote='/data/local/tmp/notemaker-ui-latency-'+uuid.uuid4().hex
    installed=False
    try:
        call('install','--no-incremental',str(apk));installed=True
        if args.compile_speed: (out/'compile.txt').write_bytes(call('shell','cmd','package','compile','-m','speed','-f',PACKAGE,timeout=180))
        call('push',str(replica),remote)
        call('shell','run-as',PACKAGE,'cp','-r',remote+'/files',remote+'/shared_prefs','.')
        call('shell','input','keyevent','KEYCODE_WAKEUP');call('shell','wm','dismiss-keyguard')
        output=call('shell','am','instrument','-w','-e','action','erase-performance','-e','mode',args.mode,PACKAGE+'/com.notemaker.Integration',timeout=240).decode()
        (out/'instrumentation.txt').write_text(output)
        assert 'PASS erase-performance' in output,output
        (out/'metrics.json').write_bytes(call('shell','run-as',PACKAGE,'cat','files/erase-performance.json'))
        report=json.loads((out/'metrics.json').read_text());samples=report['samples']
        summary={'samples':len(samples),'geometry_restored':report['geometry_restored']}
        for key in ['input_to_feedback_ms','input_to_updated_draw_ms']:
            if samples and key in samples[0]:
                values=sorted(s[key] for s in samples);summary[key+'_p95']=values[int(.95*len(values))]
        print(json.dumps(summary))
    finally:
        (out/'logcat.txt').write_bytes(call('logcat','-d','-s','AndroidRuntime','NoteMaker','NoteMakerPerformance'))
        if installed: call('uninstall',PACKAGE)
        call('shell','rm','-rf',remote)
        call('shell','am','start','-n','com.notemaker/.MainActivity')


if __name__=='__main__': main()

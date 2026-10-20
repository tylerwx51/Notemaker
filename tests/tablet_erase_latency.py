#!/usr/bin/env python3
"""Measure real eraser input to updated Canvas draw in an isolated Android package."""
import argparse
import hashlib
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
    parser.add_argument('--archive',type=Path)
    parser.add_argument('--document')
    parser.add_argument('--synthetic-strokes',action='store_true',help='Measure 1,000 synthetic strokes and process resources without a note archive')
    parser.add_argument('--interaction-suite',action='store_true',help='Measure pan, zoom redraw, and tool switching on a disposable note archive')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--compile-speed',action='store_true',help='Measure explicit Android ahead-of-time compilation separately')
    parser.add_argument('--mode',choices=['partial','item'],default='partial')
    args=parser.parse_args()
    if args.synthetic_strokes and args.interaction_suite: parser.error('Choose either --synthetic-strokes or --interaction-suite')
    if not args.synthetic_strokes and (not args.archive or not args.document): parser.error('--archive and --document are required for erasing or interaction timing')
    if not os.environ.get('IN_NIX_SHELL'): parser.error('Run through nix-shell')
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False,mode=0o700)
    adb=['adb','-s',args.serial]
    def call(*cmd,**kw): return subprocess.check_output(adb+list(cmd),timeout=kw.get('timeout',60))
    metadata={'revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
              'source_changes':subprocess.check_output(['git','diff','--stat'],cwd=ROOT,text=True),
              'synthetic_strokes':args.synthetic_strokes,'compile_speed':args.compile_speed}
    for prop in ('ro.product.model','ro.build.version.sdk','ro.build.fingerprint'):
        metadata[prop]=call('shell','getprop',prop).decode().strip()
    (out/'metadata.json').write_text(json.dumps(metadata,indent=2))
    if ('package:'+PACKAGE) in call('shell','pm','list','packages',PACKAGE).decode().splitlines(): raise RuntimeError('Isolated package already exists')
    sdk=Path(os.environ['ANDROID_HOME']);build=sdk/'build-tools/36.0.0';platform=sdk/'platforms/android-36/android.jar'
    with (ROOT/'apps/android/build/notemaker.apk').open('rb') as source_apk:
        metadata['source_apk_sha256']=hashlib.file_digest(source_apk,'sha256').hexdigest()
    (out/'metadata.json').write_text(json.dumps(metadata,indent=2))
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
    if args.archive and not args.synthetic_strokes:
        with tarfile.open(args.archive) as archive: archive.extractall(capture,filter='data')
    replica=out/'replica';(replica/'files').mkdir(parents=True);(replica/'shared_prefs').mkdir()
    found=False
    for file in (capture/'files').glob('document-*.json'):
        if json.loads(file.read_text()).get('document')==args.document:
            shutil.copy(file,replica/'files'/file.name)
            journal=Path(str(file)+'.journal')
            if journal.exists(): shutil.copy(journal,replica/'files'/journal.name)
            found=True
    assert found or args.synthetic_strokes,'Document absent'
    prefs=ET.Element('map')
    for key,value in [('document',args.document or 'synthetic-performance'),('host','')]: ET.SubElement(prefs,'string',name=key).text=value
    ET.ElementTree(prefs).write(replica/'shared_prefs/settings.xml',encoding='utf-8')
    remote='/data/local/tmp/notemaker-ui-latency-'+uuid.uuid4().hex
    installed=False
    try:
        call('install','--no-incremental',str(apk));installed=True
        if args.compile_speed: (out/'compile.txt').write_bytes(call('shell','cmd','package','compile','-m','speed','-f',PACKAGE,timeout=180))
        call('push',str(replica),remote)
        sources=[remote+'/shared_prefs'] if args.synthetic_strokes else [remote+'/files',remote+'/shared_prefs']
        call('shell','run-as',PACKAGE,'cp','-r',*sources,'.')
        call('shell','input','keyevent','KEYCODE_WAKEUP');call('shell','wm','dismiss-keyguard')
        action='performance' if args.synthetic_strokes else 'interaction-performance' if args.interaction_suite else 'erase-performance'
        output=call('shell','am','instrument','-w','-e','action',action,'-e','mode',args.mode,PACKAGE+'/com.notemaker.Integration',timeout=600).decode()
        (out/'instrumentation.txt').write_text(output)
        assert 'PASS '+action in output,output
        (out/'metrics.json').write_bytes(call('shell','run-as',PACKAGE,'cat','files/'+action+'.json'))
        if args.interaction_suite:
            report=json.loads((out/'metrics.json').read_text())
            assert all(report['modes'][name]['input_to_draw']['samples']>=35 for name in ('pan','zoom_redraw','tool_switch'))
            assert report['modes']['layer_switch']['input_to_ui_draw']['samples']>=35
            assert report['modes']['document_reopen']['input_to_draw']['samples']>=10
            print(json.dumps({name:report['modes'][name]['input_to_ui_draw'] for name in report['modes']}))
            return
        if args.synthetic_strokes:
            (out/'resources.json').write_bytes(call('shell','run-as',PACKAGE,'cat','files/performance-resources.json'))
            report=json.loads((out/'metrics.json').read_text())
            resources=json.loads((out/'resources.json').read_text())['samples']
            assert report['strokes']==1000 and report['incremental_matches_replay']
            assert [sample['strokes'] for sample in resources]==list(range(0,1001,100))
            assert all(sample['pss_kib']>0 for sample in resources)
            call('shell','am','force-stop',PACKAGE)
            reopened=call('shell','am','instrument','-w','-e','action','state',PACKAGE+'/com.notemaker.Integration',timeout=120).decode()
            (out/'reopened.txt').write_text(reopened)
            assert 'PASS state' in reopened,reopened
            state=json.loads(call('shell','run-as',PACKAGE,'cat','files/integration.json'))
            assert state['strokes']==1000
            print(json.dumps({key:report[key] for key in ('stroke_latency','drain_ms','max_save_queue')}))
            return
        report=json.loads((out/'metrics.json').read_text());samples=report['samples']
        summary={'samples':len(samples),'geometry_restored':report['geometry_restored']}
        for key in ['input_to_feedback_ms','input_to_updated_draw_ms']:
            if samples and key in samples[0]:
                values=sorted(s[key] for s in samples);summary[key+'_p95']=values[int(.95*len(values))]
        print(json.dumps(summary))
    finally:
        if installed and args.synthetic_strokes and not (out/'metrics.json').exists():
            for name,local in [('performance.json','metrics.json'),('performance-resources.json','resources.json')]:
                try: (out/local).write_bytes(call('shell','run-as',PACKAGE,'cat','files/'+name))
                except subprocess.CalledProcessError: pass
        (out/'logcat.txt').write_bytes(call('logcat','-d','-s','AndroidRuntime','NoteMaker','NoteMakerPerformance'))
        if installed: call('uninstall',PACKAGE)
        call('shell','rm','-rf',remote)
        call('shell','am','start','-n','com.notemaker/.MainActivity')


if __name__=='__main__': main()

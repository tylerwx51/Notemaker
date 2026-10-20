#!/usr/bin/env python3
"""Measure real eraser input to updated Canvas draw in an isolated Android package."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tarfile
import time
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
    parser.add_argument('--interaction-suite',action='store_true',help='Measure editor navigation, tools, layers, reopen, history, and page switching on disposable notes')
    parser.add_argument('--cold-start',action='store_true',help='Measure process-cold opens of a copied note')
    parser.add_argument('--checkpoint-performance',action='store_true',help='Measure checkpoint write cost on a copied note')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--compile-speed',action='store_true',help='Measure explicit Android ahead-of-time compilation separately')
    parser.add_argument('--seed-proxy-config',action='store_true',help='Seed an isolated, offline proxy document index for cold-start measurement')
    parser.add_argument('--background-checkpoint',action='store_true',help='Before cold-start samples, background the editor and wait for its automatic checkpoint')
    parser.add_argument('--mode',choices=['partial','item'],default='partial')
    args=parser.parse_args()
    if sum((args.synthetic_strokes,args.interaction_suite,args.cold_start,args.checkpoint_performance))>1: parser.error('Choose one performance mode')
    if args.background_checkpoint and not args.cold_start: parser.error('--background-checkpoint requires --cold-start')
    if not args.synthetic_strokes and (not args.archive or not args.document): parser.error('--archive and --document are required for erasing or interaction timing')
    if not os.environ.get('IN_NIX_SHELL'): parser.error('Run through nix-shell')
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False,mode=0o700)
    adb=['adb','-s',args.serial]
    def call(*cmd,**kw): return subprocess.check_output(adb+list(cmd),timeout=kw.get('timeout',60))
    metadata={'revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
              'source_changes':subprocess.check_output(['git','diff','--stat'],cwd=ROOT,text=True),
              'synthetic_strokes':args.synthetic_strokes,'cold_start':args.cold_start,
              'compile_speed':args.compile_speed,'seed_proxy_config':args.seed_proxy_config,
              'background_checkpoint':args.background_checkpoint}
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
    with apk.open('rb') as isolated_apk:
        metadata['isolated_apk_sha256']=hashlib.file_digest(isolated_apk,'sha256').hexdigest()
    (out/'metadata.json').write_text(json.dumps(metadata,indent=2))
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
    if args.seed_proxy_config:
        assert args.cold_start and found,'--seed-proxy-config requires --cold-start and a copied note'
        (replica/'files'/'proxy-config.json').write_text(json.dumps({'id':str(uuid.uuid4()),
            'token':uuid.uuid4().hex+uuid.uuid4().hex,'documents':[args.document]}))
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
        if args.cold_start:
            checkpoint_ms=None;checkpoint_base_bytes=None
            if args.background_checkpoint:
                journal=next((replica/'files').glob('document-*.json.journal'))
                relative='files/'+journal.name
                def journal_size(): return int(call('shell','run-as',PACKAGE,'stat','-c','%s',relative).decode().strip())
                assert journal_size()>=1024*1024,'Checkpoint fixture WAL is too small'
                launch=call('shell','am','start','-W','-n',PACKAGE+'/com.notemaker.MainActivity',timeout=120).decode()
                assert 'Status: ok' in launch,launch
                assert journal_size()>=1024*1024,'Checkpoint ran while editor was visible'
                begun=time.monotonic_ns()
                call('shell','input','keyevent','KEYCODE_HOME')
                deadline=time.monotonic()+30
                while journal_size()!=0 and time.monotonic()<deadline: time.sleep(.1)
                assert journal_size()==0,'Background checkpoint did not finish'
                checkpoint_ms=(time.monotonic_ns()-begun)/1e6
                checkpoint_base_bytes=int(call('shell','run-as',PACKAGE,'stat','-c','%s',
                    'files/'+journal.name.removesuffix('.journal')).decode().strip())
            ticks=int(call('shell','getconf','CLK_TCK').decode().strip())
            samples=[]
            for index in range(12):
                call('shell','am','force-stop',PACKAGE)
                time.sleep(.2)
                stopped=subprocess.run(adb+['shell','pidof',PACKAGE],capture_output=True,timeout=30)
                assert not stopped.stdout.strip(),f'Package remained running after force-stop: {stopped.stdout!r}'
                begun=time.monotonic_ns()
                launch=call('shell','am','start','-W','-n',PACKAGE+'/com.notemaker.MainActivity',timeout=120).decode()
                host_ms=(time.monotonic_ns()-begun)/1e6
                fields=dict(re.findall(r'^([A-Za-z]+):\s*(.+)$',launch,re.M))
                assert fields.get('Status')=='ok' and fields.get('LaunchState')=='COLD',launch
                pid=call('shell','pidof',PACKAGE).decode().strip()
                assert pid.isdigit(),pid
                io_text=call('shell','run-as',PACKAGE,'cat',f'/proc/{pid}/io').decode()
                io={key:int(value) for key,value in re.findall(r'^(\w+):\s*(\d+)$',io_text,re.M)}
                stat=call('shell','run-as',PACKAGE,'cat',f'/proc/{pid}/stat').decode()
                cpu_fields=stat[stat.rfind(')')+2:].split()
                cpu_ms=(int(cpu_fields[11])+int(cpu_fields[12]))*1000/ticks
                memory=call('shell','dumpsys','meminfo',PACKAGE).decode()
                pss=re.search(r'TOTAL PSS:\s*(\d+)',memory)
                assert pss, memory[-2000:]
                samples.append({'index':index,'android_total_ms':int(fields['TotalTime']),
                                'android_wait_ms':int(fields['WaitTime']),
                                'host_start_wall_ms':host_ms,'process_cpu_ms':cpu_ms,
                                'pss_kib':int(pss.group(1)),'io':io})
            (out/'metrics.json').write_text(json.dumps({'warmup':2,'background_checkpoint_ms':checkpoint_ms,
                'background_checkpoint_base_bytes':checkpoint_base_bytes,'samples':samples},indent=2)+'\n')
            timed=sorted(sample['android_total_ms'] for sample in samples[2:])
            print(json.dumps({'samples':len(timed),'android_total_p50_ms':statistics.median(timed),
                              'android_total_p95_ms':timed[int(.95*len(timed))]}))
            return
        if args.checkpoint_performance:
            output=call('shell','am','instrument','-w','-e','action','checkpoint-performance',PACKAGE+'/com.notemaker.Integration',timeout=180).decode()
            (out/'instrumentation.txt').write_text(output)
            assert 'PASS checkpoint-performance' in output,output
            (out/'metrics.json').write_bytes(call('shell','run-as',PACKAGE,'cat','files/checkpoint-performance.json'))
            print((out/'metrics.json').read_text())
            return
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
            assert report['modes']['history_toggle']['input_to_draw']['samples']>=54
            assert report['modes']['page_switch']['input_to_draw']['samples']>=35
            assert all(report['history_directions'][direction]['input_to_ui_draw']['samples']==27 for direction in ('undo','redo'))
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
        (out/'logcat.txt').write_bytes(call('logcat','-d','-s','AndroidRuntime','NoteMaker','NoteMakerPerformance','NoteMakerStartup'))
        if installed: call('uninstall',PACKAGE)
        call('shell','rm','-rf',remote)
        call('shell','am','start','-n','com.notemaker/.MainActivity')


if __name__=='__main__': main()

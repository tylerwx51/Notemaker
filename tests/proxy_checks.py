#!/usr/bin/env python3
"""Actual CLI clients sharing the cached proxy, journal crash recovery and actor history."""
import concurrent.futures
import json
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from cli_checks import CLI, EXAMPLE
import server_checks
from topology_support import Link, Proxy, eventually, http

class ProxyChecks(server_checks.ServerChecks):
    def test_checkpoint_deduplicates_only_reconstructable_relay(self):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp);proxy=Proxy(directory,'relay-dedup')
            try:
                proxy.cli('document','create','pages')
                proxy.cli('object','put','-',value={'object':'one','shape':'rect','x':10})
                snapshot=proxy.cli('log')
                proxy.cli('checkpoint')
                base=next(directory.glob('document-*.json'))
                self.assertEqual(json.loads(base.read_text())['relay'],[])
            finally:proxy.stop()
            # A retained ingress entry outside the current event/pending lists
            # must survive a later checkpoint rather than being reconstructed.
            extra={'id':'relay-only','kind':'document','type':'pages'}
            persisted=json.loads(base.read_text())
            persisted['relay']=snapshot['events']+snapshot['pending']+[extra]
            base.write_text(json.dumps(persisted))
            proxy=Proxy(directory,'relay-dedup')
            try:
                before=proxy.cli('log')
                self.assertEqual(before['relay'][-1],extra)
                proxy.cli('settings','-',value={'selected':'base'})
                expected=proxy.cli('log')
                proxy.cli('checkpoint')
                self.assertEqual(json.loads(base.read_text())['relay'][-1],extra)
                self.assertEqual(proxy.cli('log'),expected)
            finally:proxy.stop()

    def test_checkpoint_preserves_reordered_relay_with_id_order(self):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp);proxy=Proxy(directory,'relay-order')
            try:
                proxy.cli('document','create','pages')
                proxy.cli('object','put','-',value={'object':'one','shape':'rect','x':10})
                proxy.cli('object','put','-',value={'object':'two','shape':'rect','x':20})
                initial=proxy.cli('log');proxy.cli('checkpoint')
                base=next(directory.glob('document-*.json'))
            finally:proxy.stop()
            reordered=list(reversed(initial['events']+initial['pending']))
            self.assertGreater(len(reordered),1)
            persisted=json.loads(base.read_text());persisted['relay']=reordered;base.write_text(json.dumps(persisted))
            proxy=Proxy(directory,'relay-order')
            try:
                self.assertEqual(proxy.cli('log')['relay'],reordered)
                proxy.cli('settings','-',value={'selected':'base'})
                expected=proxy.cli('log');before=base.stat().st_size
                proxy.cli('checkpoint')
                compact=json.loads(base.read_text())
                self.assertEqual(compact['relayFormat'],'ids')
                self.assertEqual(compact['relay'],[op['id'] for op in reordered])
                self.assertLess(base.stat().st_size,before)
                self.assertEqual(proxy.cli('log'),expected)
            finally:proxy.stop()
            proxy=Proxy(directory,'relay-order')
            try:self.assertEqual(proxy.cli('log'),expected)
            finally:proxy.stop()

    def test_json_string_fast_path_and_escapes_round_trip(self):
        with tempfile.TemporaryDirectory() as temp:
            proxy=Proxy(Path(temp),'json-strings');self.addCleanup(proxy.stop)
            values=['ordinary text','quote " and slash \\','line\nfeed','tab\tand snowman ☃']
            proxy.cli('settings','-',value={'undo':values,'selected':values[1]})
            saved=proxy.cli('log')
            self.assertEqual(saved['undo'],values)
            self.assertEqual(saved['selected'],values[1])
            invalid=subprocess.run([str(CLI),'--proxy-file',str(proxy.descriptor_path),
                                    '--document','json-strings','settings','-'],
                                   input='{"selected":"raw\nnewline"}',text=True,capture_output=True,timeout=30)
            self.assertNotEqual(invalid.returncode,0)

    def test_checkpoint_preserves_history_and_skips_pretruncate_journal(self):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp);proxy=Proxy(directory,'checkpoint-history')
            def cli(*args,value=None,actor='writer',remote=True):
                location=['--proxy-file',str(proxy.descriptor_path)] if remote else ['--state',str(directory)]
                result=subprocess.run([str(CLI),*location,'--document','checkpoint-history','--actor',actor,*args],
                                      input=json.dumps(value) if value is not None else None,
                                      text=True,capture_output=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                return json.loads(result.stdout)
            try:
                cli('document','create','pages')
                cli('object','put','-',value={'object':'before','shape':'rect','x':10})
                baseline=cli('log');journal=next(directory.glob('*.journal'))
                original=journal.read_bytes();self.assertTrue(original)
                result=cli('checkpoint')
                self.assertEqual(result['revision'],baseline['revision'])
                self.assertEqual(result['journalBytesBefore'],len(original))
                self.assertEqual(journal.stat().st_size,0)
                self.assertEqual(cli('log'),baseline)
                # Simulate a crash after the base rename but before WAL truncation.
                journal.write_bytes(original)
            finally:proxy.stop()
            self.assertEqual(cli('log',remote=False),baseline)
            proxy=Proxy(directory,'checkpoint-history')
            try:
                cli('object','put','-',value={'object':'after','shape':'rect','x':20})
                cli('undo');self.assertFalse(cli('inspect')['objectActive']['after'])
                cli('undo');self.assertFalse(cli('inspect')['objectActive']['before'])
                cli('redo');self.assertTrue(cli('inspect')['objectActive']['before'])
            finally:proxy.stop()
            self.assertEqual(len(cli('inspect',remote=False)['objects']),2)

    def test_editor_undo_list_journal_stays_linear_and_replays(self):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp);proxy=Proxy(directory,'editor-history-growth')
            self.addCleanup(proxy.stop)
            def cli(*args,value=None):
                command=[str(CLI),'--proxy-file',str(proxy.descriptor_path),'--document','editor-history-growth',*args]
                result=subprocess.run(command,input=json.dumps(value) if value is not None else None,
                                      text=True,capture_output=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                return json.loads(result.stdout)
            for i in range(40):
                cli('settings','-',value={'undo':[f'stroke-{j}' for j in range(i+1)],'redo':[]})
            import struct
            data=next(directory.glob('*.journal')).read_bytes();lengths=[];offset=0
            while offset<len(data):
                length=struct.unpack_from('>i',data,offset)[0];lengths.append(length);offset+=length+8
            self.assertEqual(len(lengths),40)
            self.assertLess(lengths[-1],lengths[0]*3,'new setting rewrote the full undo list')
            proxy.stop()
            restarted=subprocess.run([str(CLI),'--state',str(directory),'--document','editor-history-growth','log'],
                                     text=True,capture_output=True,timeout=30)
            self.assertEqual(restarted.returncode,0,restarted.stderr)
            self.assertEqual(json.loads(restarted.stdout)['undo'][-1],'stroke-39')

    def test_actor_history_journal_stays_linear_and_replays_after_restart(self):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp)
            proxy=Proxy(directory,'history-growth')
            self.addCleanup(proxy.stop)
            def cli(*args,value=None):
                command=[str(CLI),'--proxy-file',str(proxy.descriptor_path),'--document','history-growth','--actor','writer',*args]
                result=subprocess.run(command,input=json.dumps(value) if value is not None else None,
                                      text=True,capture_output=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                return json.loads(result.stdout)
            for i in range(40):
                cli('object','put','-',value={'object':f'item-{i}','shape':'rect','x':i*10})
            import struct
            data=next(directory.glob('*.journal')).read_bytes()
            lengths=[];offset=0
            while offset<len(data):
                length=struct.unpack_from('>i',data,offset)[0]
                lengths.append(length)
                offset+=length+8
            self.assertEqual(len(lengths),40)
            self.assertLess(lengths[-1],lengths[0]*3,'new action rewrote full actor history')
            self.assertLess(sum(lengths),lengths[0]*40*2)
            proxy.stop()
            proxy=Proxy(directory,'history-growth')
            self.addCleanup(proxy.stop)
            self.assertEqual(len(cli('inspect')['objects']),40)
            cli('undo')
            self.assertFalse(cli('inspect')['objectActive']['item-39'])
            cli('redo')
            self.assertTrue(cli('inspect')['objectActive']['item-39'])

    def test_active_edit_preempts_delayed_idle_document_fetch(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            self.post('idle-document',[dict(id='idle-seed',kind='document',type='fixed')])
            self.post('live-document',[dict(id='live-seed',kind='document',type='fixed')])
            link=Link(self.url,root/'link.json')
            link.delay_next_replica('idle-document',4)
            proxy=None
            try:
                proxy=Proxy(root/'replica','idle-document',link.url)
                self.assertTrue(link.delay_started.wait(8),'sync loop never entered delayed idle-document fetch')
                begin=time.monotonic()
                http(proxy.descriptor['url']+'/command',dict(document='live-document',actor='agent',args=['append','-'],input={'kind':'document','type':'fixed'}),proxy.descriptor['token'])
                events=eventually(lambda:[row for row in self.read('live-document') if row['op'].get('type')=='fixed' and row['op'].get('actor')=='agent'],3)
                self.assertLess(time.monotonic()-begin,3.0,'active document waited behind the delayed idle-document fetch')
                self.assertTrue(events)
                def idle_caught_up():
                    state=http(proxy.descriptor['url']+'/command',dict(document='idle-document',actor='observer',args=['snapshot']),proxy.descriptor['token'])['result']
                    return state['cursor']>=1
                eventually(idle_caught_up,10)
            finally:
                if proxy is not None:proxy.stop()
                link.close()

    def test_large_catchup_uses_bounded_journal_transactions(self):
        points=[[i/3,(i%1400)/3,.54321] for i in range(6000)]
        for i in range(20):
            self.post('large-catchup',[dict(id=f'large-{i}',kind='stroke',stroke=f'large-{i}',layer='base',page=0,color=-16777216,width=5,points=points)])
        with tempfile.TemporaryDirectory() as directory:
            proxy=Proxy(directory,'large-catchup');self.addCleanup(proxy.stop)
            def cli(*args):
                p=subprocess.run([str(CLI),'--proxy-file',str(proxy.descriptor_path),'--document','large-catchup',*args],text=True,capture_output=True,timeout=30)
                self.assertEqual(p.returncode,0,p.stderr);return json.loads(p.stdout)
            cli('sync',self.url)
            self.assertEqual(len(cli('log')['events']),20)
            self.assertEqual(len(cli('inspect')['strokes']),20)
            self.assertEqual(cli('sync',self.url)['downloaded'],0)
            import struct
            data=next(Path(directory).glob('*.journal')).read_bytes();offset=0;records=0
            while offset<len(data):
                size=struct.unpack_from('>i',data,offset)[0];self.assertLess(size,3100000);offset+=size+8;records+=1
            self.assertGreaterEqual(records,3)

    def test_proxy_offline_concurrent_history_reconnect_and_restart(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);directory=root/'replica';descriptor=directory/'proxy.json'
            daemon=subprocess.Popen([str(CLI),'--state',str(directory),'serve','0'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            try:
                ready=json.loads(daemon.stdout.readline());self.assertTrue(ready['port'])
                def call(*args,actor='agent',value=None,proxy=True):
                    options=['--proxy-file',str(descriptor)] if proxy else ['--state',str(directory)]
                    p=subprocess.run([str(CLI),*options,'--document','proxy-test','--actor',actor,*args],input=json.dumps(value) if value is not None else None,text=True,capture_output=True,timeout=30)
                    self.assertEqual(p.returncode,0,p.stderr);return json.loads(p.stdout)
                call('document','create','pages')
                call('diagram','add','-',value=EXAMPLE)
                svg=Path(__file__).resolve().parents[1]/'apps/cli/examples/placed.svg'
                placement=root/'placement.json';placement.write_text(json.dumps({'object':'proxy-svg','x':55,'y':66,'page':1}))
                call('svg','add',str(svg),str(placement))
                self.assertEqual(call('inspect')['objects'][-1]['svg'],svg.read_text())
                self.assertEqual(call('inspect')['objects'][-1]['x'],55)
                call('undo');self.assertFalse(call('inspect')['objectActive']['proxy-svg'])
                call('redo');self.assertTrue(call('inspect')['objectActive']['proxy-svg'])
                def edit(i):return call('object','put','-',actor=f'agent-{i}',value={'object':f'node-{i}','shape':'rect','x':i*10})
                with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:list(pool.map(edit,range(24)))
                call('undo',actor='agent-3');model=call('inspect');self.assertFalse(model['objectActive']['node-3']);self.assertTrue(model['objectActive']['node-4'])
                call('redo',actor='agent-3');self.assertTrue(call('inspect')['objectActive']['node-3'])
                unchanged=call('log')
                bad=subprocess.run([str(CLI),'--proxy-file',str(descriptor),'--document','proxy-test','apply','-'],input=json.dumps([{'kind':'layer','layer':'bad','name':'Bad','force':False},{'kind':'object','object':'incomplete'}]),text=True,capture_output=True,timeout=30)
                self.assertNotEqual(bad.returncode,0);self.assertEqual(call('log'),unchanged)
                call('export',str(root/'proxy.svg'));self.assertIn('NoteMaker app',(root/'proxy.svg').read_text())
                call('object','update','app','-',value={'scope':{'layer':'architecture','page':0},'x':432})
                self.assertEqual(call('inspect')['objects'][0]['x'],432)
                before=call('inspect');call('sync',self.url)
                self.assertEqual(call('inspect'),before);self.assertEqual(call('status')['pending'],0)
                persisted=call('log');self.assertEqual(call('inspect',proxy=False),before)
            finally:
                daemon.terminate();daemon.wait(timeout=10);daemon.stdout.close();daemon.stderr.close()
            journal=next(directory.glob('*.journal'));size=journal.stat().st_size
            with journal.open('ab') as f:f.write(b'\x00\x00\x00\x64partial transaction')
            self.assertEqual(call('log',proxy=False),persisted);self.assertGreater(journal.stat().st_size,size)
            recovered=Proxy(directory,'proxy-test');recovered.command('status');recovered.stop()
            self.assertEqual(journal.stat().st_size,size)
            self.assertEqual(call('inspect',proxy=False),before)
            data=bytearray(journal.read_bytes());data[-1]^=1;journal.write_bytes(data)
            broken=subprocess.run([str(CLI),'--state',str(directory),'--document','proxy-test','inspect'],text=True,capture_output=True,timeout=30)
            self.assertNotEqual(broken.returncode,0);self.assertIn('checksum',broken.stderr)

    def test_strict_number_parser(self):
        with tempfile.TemporaryDirectory() as directory:
            proxy=Proxy(directory);self.addCleanup(proxy.stop)
            for number in ['01','-01','.1','1.','1e','1e+','+1','NaN','Infinity','1e999']:
                body='{"object":"number","shape":"rect","x":'+number+'}'
                result=subprocess.run([str(CLI),'--proxy-file',str(proxy.descriptor_path),'object','put','-'],input=body,text=True,capture_output=True,timeout=10)
                self.assertNotEqual(result.returncode,0,(number,result.stdout))
            for number in ['0','-0','1.25','-12.5e+2','1E-3']:
                body='{"object":"number","shape":"rect","x":'+number+'}'
                result=subprocess.run([str(CLI),'--proxy-file',str(proxy.descriptor_path),'object','put','-'],input=body,text=True,capture_output=True,timeout=10)
                self.assertEqual(result.returncode,0,(number,result.stderr))

if __name__=='__main__':unittest.main()

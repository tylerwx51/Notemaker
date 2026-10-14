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
from topology_support import Proxy

class ProxyChecks(server_checks.ServerChecks):
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
                call('object','update','app','-',value={'x':432})
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

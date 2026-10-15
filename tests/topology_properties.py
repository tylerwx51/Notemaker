#!/usr/bin/env python3
"""Generated topology/failure histories against real JVM proxies and the main server."""
import concurrent.futures
import json
import os
import subprocess
import tempfile
import time
import unittest
import uuid
from pathlib import Path
from urllib.error import HTTPError
from hypothesis import given, settings, strategies as st, HealthCheck
import server_checks
from topology_support import Proxy, Link, eventually, http, CLI

class TopologyProperties(server_checks.ServerChecks):
    @settings(max_examples=int(os.environ.get('NOTEMAKER_TOPOLOGY_EXAMPLES','25')), deadline=None, suppress_health_check=[HealthCheck.too_slow])
    @given(parents=st.lists(st.integers(0, 100), min_size=1, max_size=5),
           bursts=st.lists(st.lists(st.tuples(st.integers(0, 100), st.integers(-1000, 1000), st.sampled_from(['object','stroke','diagram'])), min_size=1, max_size=5), min_size=2, max_size=4),
           crash=st.integers(0, 100), fault=st.sampled_from(['partition', 'duplicate', 'lost_response']), restart_root=st.booleans())
    def test_generated_trees_converge(self, parents, bursts, crash, fault, restart_root):
        with tempfile.TemporaryDirectory() as temp:
            directory=Path(temp); proxies=[]; links=[]; acknowledged=set(); doc='tree-'+uuid.uuid4().hex; root_stopped=False
            def restore_root():
                cls=type(self)
                cls.process=subprocess.Popen(cls.process.args,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                eventually(lambda: http(self.url+'/api/info'))
            try:
                for i, choice in enumerate(parents):
                    parent=choice % (i+1)-1
                    upstream=self.url if parent < 0 else proxies[parent].descriptor['url']
                    token=None if parent < 0 else proxies[parent].descriptor['token']
                    link=Link(upstream, directory/f'link-{i}.json', token); links.append(link)
                    proxy=Proxy(directory/str(i), doc, link.target); proxies.append(proxy)
                # Every topology has a real CLI client and a direct local reader.
                result=proxies[0].cli('object','put','-',value={'object':'shared','shape':'rect','x':0})
                acknowledged.add(result['operation'])
                self.assertEqual(proxies[0].cli('inspect',direct=True)['objects'][0]['x'],0)
                if restart_root:
                    self.process.kill(); self.process.wait(timeout=5); root_stopped=True
                selected=links[crash % len(links)]
                if fault=='partition': selected.disconnect()
                elif fault=='duplicate': selected.duplicate=True
                else: selected.drop=2
                def edit(item):
                    n, x, kind=item; proxy=proxies[n % len(proxies)]
                    if kind=='diagram':
                        identity=uuid.uuid4().hex
                        return proxy.command('diagram','add','-',actor='writer-'+str(n),value={'name':'Atomic','layer':'diagram-'+identity,'objects':[{'object':identity+'a','shape':'rect','x':x},{'object':identity+'b','shape':'ellipse','x':x+10}]})['operation']
                    if kind=='stroke':
                        return proxy.command('stroke','put','-',actor='writer-'+str(n),value={'stroke':'ink','layer':'base','page':0,'color':-16777216,'width':3,'points':[[x,100,.5]]})['operation']
                    # Competing geometry plus independent actor histories.
                    return proxy.command('object','put','-',actor='writer-'+str(n),value={'object':'shared','shape':'rect','x':x})['operation']
                for burst in bursts:
                    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
                        work=[pool.submit(edit,item) for item in burst]
                        # A direct reader races the writers and must never see half a diagram.
                        snapshot=proxies[crash % len(proxies)].cli('inspect',direct=True)
                        for layer in snapshot['layers']:
                            if layer['layer'].startswith('diagram-'):
                                self.assertEqual(len([o for o in snapshot['objects'] if o['layer']==layer['layer']]),2)
                        acknowledged.update(f.result() for f in work)
                    p=proxies[crash % len(proxies)]
                    before=p.cli('log',direct=True)
                    p.stop(crash=True); p.start()
                    after=p.cli('log',direct=True)
                    before_ids={op['id'] for op in before['events']+before['pending']}
                    after_ids={op['id'] for op in after['events']+after['pending']}
                    self.assertLessEqual(before_ids,after_ids)
                selected.online=True
                if root_stopped: restore_root(); root_stopped=False
                def converged():
                    states=[p.command('log') for p in proxies]
                    if any(s['pending'] for s in states): return False
                    events=[r['op'] for r in self.read(doc)]
                    if {e['id'] for e in events} != acknowledged: return False
                    return states if all(s['events']==events for s in states) else False
                states=eventually(converged, timeout=65)
                models=[p.command('inspect') for p in proxies]
                self.assertTrue(all(m==models[0] for m in models))
                events=states[0]['events']
                self.assertEqual(len(events),len(acknowledged))
                # Independent oracle: last committed geometry wins.
                last=[a for event in events for a in event.get('actions',[event]) if a['kind']=='object' and a['object']=='shared'][-1]
                self.assertEqual(next(o for o in models[0]['objects'] if o['object']=='shared')['x'],last['x'])
            finally:
                for proxy in reversed(proxies): proxy.stop()
                for link in links: link.close()
                if root_stopped: restore_root()

    def test_cycles_sole_writer_and_offline_chain(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); proxies=[]
            try:
                for i in range(3): proxies.append(Proxy(root/str(i), upstream=None if i==0 else proxies[i-1].descriptor_path))
                with self.assertRaises(HTTPError): proxies[0].command('configure',str(proxies[2].descriptor_path))
                with self.assertRaises(HTTPError): proxies[1].command('configure',str(proxies[1].descriptor_path))
                with self.assertRaises(HTTPError): proxies[1].command('configure',proxies[1].descriptor['url'])
                error=proxies[0].cli('object','put','-',value={'object':'bad','shape':'rect'},direct=True,ok=False)
                self.assertIn('require',error['error'])
                duplicate=subprocess.run([str(CLI),'--state',str(root/'0'),'serve','0'],capture_output=True,text=True,timeout=5)
                self.assertNotEqual(duplicate.returncode,0)
                result=proxies[2].cli('diagram','add',str(Path(__file__).resolve().parents[1]/'apps/cli/examples/diagram.json'))
                eventually(lambda: len(proxies[0].command('inspect')['objects'])==8)
                for proxy in proxies:
                    self.assertEqual(proxy.command('log')['events'],[])
                # Parent outage must not prevent its child accepting local actions.
                proxies[0].stop(crash=True)
                proxies[2].command('object','update','app','-',value={'scope':{'layer':'architecture','page':0},'x':432})
                eventually(lambda: proxies[1].command('inspect')['objects'][0]['x']==432)
                proxies[0].start()
                proxies[0].command('configure',self.url)
                eventually(lambda: all(p.command('status')['pending']==0 for p in proxies),timeout=40)
                self.assertTrue(all(p.command('inspect')==proxies[0].command('inspect') for p in proxies))
                before=proxies[0].command('log')
                with self.assertRaises(HTTPError):
                    http(proxies[0].descriptor['url']+'/api/events',dict(document='property',rootId='different-server',operations=[{'id':'foreign','kind':'layer','layer':'foreign','name':'Foreign','force':False}]),proxies[0].descriptor['token'])
                self.assertEqual(proxies[0].command('log'),before)
                proxies[2].command('undo')
                eventually(lambda: all(p.command('inspect')['objects'][0]['x']!=432 for p in proxies))
            finally:
                for p in reversed(proxies): p.stop()

    def test_batch_deadline_rate_bounds_and_idle(self):
        with tempfile.TemporaryDirectory() as temp:
            link=Link(self.url,Path(temp)/'link.json'); proxy=Proxy(Path(temp)/'replica','rate-'+uuid.uuid4().hex,link.target)
            try:
                eventually(lambda: proxy.command('status')['serverId'] is not None)
                time.sleep(.6); started=time.monotonic()
                # Continuous input spans several windows; it must not debounce forever.
                for i in range(30):
                    proxy.command('object','put','-',value={'object':str(i),'shape':'rect'})
                    time.sleep(.035)
                eventually(lambda: proxy.command('status')['pending']==0)
                self.assertGreaterEqual(len(link.uploads),2)
                self.assertGreaterEqual(link.uploads[0][0]-started,.45)
                self.assertLess(link.uploads[0][0]-started,1.0)
                self.assertTrue(all(b[0]-a[0]>=.48 for a,b in zip(link.uploads,link.uploads[1:])))
                self.assertTrue(all(len(body['operations'])<=100 and size<=3_010_000 for _,body,size in link.uploads))
                self.assertEqual(link.max_active_uploads,1)
                time.sleep(.8); reads=len(link.reads); uploads=len(link.uploads)
                time.sleep(1.2)
                self.assertEqual(len(link.reads),reads,'Idle replicas must not poll')
                self.assertEqual(len(link.uploads),uploads)
            finally: proxy.stop(); link.close()

    def test_backlog_enforces_action_and_byte_caps(self):
        with tempfile.TemporaryDirectory() as temp:
            link=Link(self.url,Path(temp)/'link.json'); proxy=Proxy(Path(temp)/'replica','backlog-'+uuid.uuid4().hex)
            try:
                for i in range(205): proxy.command('object','put','-',value={'object':str(i),'shape':'rect'})
                for i in range(20):
                    proxy.command('append','-',value={'kind':'stroke','stroke':'large-'+str(i),'layer':'base','page':0,'color':-16777216,'width':3,'points':[[123456.789,987654.321,.555555]]*6000})
                proxy.command('configure',link.target)
                eventually(lambda: proxy.command('status')['pending']==0,timeout=40)
                self.assertGreaterEqual(len(link.uploads),4)
                self.assertEqual(link.max_active_uploads,1)
                self.assertEqual(sum(len(body['operations']) for _,body,_ in link.uploads),225)
                for _,body,_ in link.uploads:
                    self.assertLessEqual(len(body['operations']),100)
                    self.assertLessEqual(sum(len(json.dumps(op,separators=(',',':')).encode()) for op in body['operations']),3_000_000)
                self.assertTrue(all(b[0]-a[0]>=.48 for a,b in zip(link.uploads,link.uploads[1:])))
            finally: proxy.stop(); link.close()

    def test_opening_an_existing_document_wakes_idle_proxy(self):
        doc='existing-'+uuid.uuid4().hex
        self.post(doc,[dict(id='existing',kind='layer',layer='existing',name='Existing',force=False)])
        with tempfile.TemporaryDirectory() as temp:
            proxy=Proxy(Path(temp)/'replica',upstream=self.url)
            try:
                eventually(lambda: proxy.command('status')['serverId'] is not None)
                time.sleep(.6)
                proxy.document=doc
                proxy.command('inspect')
                eventually(lambda: len(proxy.command('inspect')['layers'])==2)
                proxy.stop(crash=True);proxy.start()
                self.assertEqual(len(proxy.cli('inspect',direct=True)['layers']),2)
            finally: proxy.stop()

if __name__=='__main__': unittest.main()

#!/usr/bin/env python3
"""Selection lease arbitration, mutation guards and transient previews via real CLI replicas."""
import json
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
import urllib.error

from server_checks import ServerChecks
from topology_support import Proxy, http

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'apps/cli/notemaker'


class SelectionChecks(ServerChecks):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory(prefix='notemaker-selection-')
        self.root = Path(self.tempdir.name)
        self.document = 'selection-' + self.id().split('.')[-1]
        self.proxies = []

    def tearDown(self):
        for proxy in self.proxies:
            proxy.stop()
        self.tempdir.cleanup()

    def proxy(self, name, upstream=None):
        proxy = Proxy(self.root / name, self.document, upstream)
        self.proxies.append(proxy)
        return proxy

    def call(self, proxy, *args, actor='alice', value=None, ok=True):
        result = subprocess.run([str(CLI), '--proxy-file', str(proxy.descriptor_path),
                                 '--document', self.document, '--actor', actor, *args],
                                input=json.dumps(value) if value is not None else None,
                                text=True, capture_output=True, timeout=40)
        self.assertEqual(result.returncode == 0, ok, result.stderr or result.stdout)
        return json.loads(result.stdout if ok else result.stderr)

    def locks(self):
        return http(self.url + '/api/locks?document=' + self.document)['leases']

    def test_sibling_selection_race_and_disconnect_propagates(self):
        a = self.proxy('selection-a', self.url)
        b = self.proxy('selection-b', self.url)
        self.call(a, 'object', 'put', '-', value={'object':'race-object','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(a, 'sync', self.url)
        self.call(b, 'sync', self.url)
        answers=[]
        import concurrent.futures
        def acquire(proxy, actor, session):
            return proxy,actor,self.call(proxy,'selection','select','-',actor=actor,
                value={'session':session,'page':0,'entities':[{'kind':'object','id':'race-object'}]})
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures=[pool.submit(acquire,a,'alice','session-a'),pool.submit(acquire,b,'bob','session-b')]
            answers=[future.result() for future in futures]
        self.assertEqual(sum(len(answer[2]['selected']) for answer in answers),1,answers)
        self.assertEqual(sum(len(answer[2]['skipped']) for answer in answers),1,answers)
        owner=next(answer for answer in answers if answer[2]['selected'])
        self.assertGreater(self.call(owner[0],'selection','disconnect',actor=owner[1])['released'],0)
        self.assertEqual(self.locks(),{})

    def test_page_one_layer_lock_blocks_hide_and_undo(self):
        a=self.proxy('page-lock',self.url)
        self.call(a,'document','create','pages')
        self.call(a,'layer','put','-',value={'layer':'page-layer','name':'Page Layer','force':False})
        self.call(a,'object','put','-',value={'object':'page-one','layer':'page-layer','shape':'rect','x':5,'y':5,'w':50,'h':40,'page':1})
        self.call(a,'selection','select','-',value={'session':'page-session','layer':'page-layer','page':1,'entities':[{'kind':'object','id':'page-one'}]})
        self.assertIn('1:object:page-one',self.locks(),self.locks())
        with self.assertRaises(urllib.error.HTTPError):
            self.post(self.document,[{'id':'remote-page-hide','kind':'object-active','object':'page-one','active':False,'actor':'bob'}])
        self.call(a,'layer','hide','page-layer',actor='bob',ok=False)
        self.assertTrue(self.call(a,'inspect')['objectActive']['page-one'])
        self.call(a,'layer','hide','page-layer',actor='alice')
        self.call(a,'undo',actor='alice')
        self.call(a,'layer','hide','page-layer',actor='bob',ok=False)
        self.call(a,'selection','deselect','page-session',actor='alice')
        self.call(a,'layer','hide','page-layer',actor='bob')

    def test_preview_is_ephemeral_cancellable_and_expires_with_lease(self):
        a=self.proxy('preview')
        self.call(a,'object','put','-',value={'object':'preview-object','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(a,'selection','select','-',value={'session':'preview-session','page':0,'entities':[{'kind':'object','id':'preview-object'}]})
        self.call(a,'transform','preview','-',value={'session':'preview-session','entities':[{'kind':'object','id':'preview-object','page':0}],'transform':{'dx':20}})
        self.assertIn('preview-session',self.call(a,'snapshot')['previews'])
        self.assertEqual(len(self.call(a,'log')['pending']),1)
        self.call(a,'transform','cancel','preview-session')
        self.assertNotIn('preview-session',self.call(a,'snapshot')['previews'])
        self.call(a,'transform','preview','-',value={'session':'preview-session','entities':[{'kind':'object','id':'preview-object','page':0}],'transform':{'dx':20}})
        time.sleep(15.5)
        self.assertNotIn('preview-session',self.call(a,'snapshot')['previews'])
        self.assertEqual(self.locks(),{})

    def test_cross_proxy_preview_clears_on_deselect_and_commit(self):
        parent=self.proxy('preview-parent',self.url)
        child=self.proxy('preview-child',str(parent.descriptor_path))
        self.call(parent,'object','put','-',value={'object':'shared-preview','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(child,'sync')
        entity={'kind':'object','id':'shared-preview','page':0}
        self.call(child,'selection','select','-',actor='child-user',value={'session':'child-near','page':0,'entities':[entity]})
        lease_key='0:object:shared-preview';short=dict(self.call(child,'snapshot')['leases'][lease_key]);short['expires']=int(time.time()*1000)+1200
        http(child.descriptor['url']+'/api/locks',{'document':self.document,'actor':'child-user','session':'child-near','leases':{lease_key:short},'remove':[]},child.descriptor['token'])
        near={'session':'child-near','entities':[entity],'transform':{'dx':10}}
        spoof={'session':'spoof-session','actor':'other-user','entities':[entity],'transform':{'dx':99},'expires':int(time.time()*1000)+500}
        http(self.url+'/api/locks',{'document':self.document,'actor':'other-user','previews':{'spoof-session':spoof}})
        self.assertNotIn('spoof-session',http(self.url+'/api/locks?document='+self.document)['previews'])
        self.call(child,'transform','preview','-',actor='child-user',value=near)
        child_preview=self.call(parent,'snapshot')['previews']['child-near']
        self.assertLessEqual(child_preview['expires'],short['expires'])
        time.sleep(1.3)
        self.assertNotIn('child-near',self.call(parent,'snapshot')['previews'])
        self.call(child,'selection','select','-',actor='child-user',value={'session':'child-deselect','page':0,'entities':[entity]})
        preview={'session':'child-deselect','entities':[entity],'transform':{'dx':15}}
        self.call(child,'transform','preview','-',actor='child-user',value=preview)
        self.assertIn('child-deselect',self.call(parent,'snapshot')['previews'])
        self.call(child,'selection','deselect','child-deselect',actor='child-user')
        self.assertNotIn('child-deselect',self.call(parent,'snapshot')['previews'])
        self.call(child,'selection','select','-',actor='child-user',value={'session':'child-commit','page':0,'entities':[entity]})
        preview['session']='child-commit'
        self.call(child,'transform','preview','-',actor='child-user',value=preview)
        self.assertIn('child-commit',self.call(parent,'snapshot')['previews'])
        self.call(child,'transform','commit','-',actor='child-user',value=preview)
        self.assertNotIn('child-commit',self.call(parent,'snapshot')['previews'])
        self.assertIn('0:object:shared-preview',self.call(parent,'snapshot')['leases'])

    def test_transform_commit_uploads_without_an_explicit_sync_command(self):
        parent=self.proxy('auto-upload-parent',self.url)
        child=self.proxy('auto-upload-child',str(parent.descriptor_path))
        self.call(parent,'object','put','-',value={'object':'auto-upload-object','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(parent,'sync',self.url)
        self.call(child,'sync')
        entity={'kind':'object','id':'auto-upload-object','page':0}
        self.call(child,'selection','select','-',value={'session':'auto-upload-session','page':0,'entities':[entity]})
        self.call(child,'transform','commit','-',value={'session':'auto-upload-session','entities':[entity],'transform':{'dx':31,'dy':12}})
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            for row in self.read(self.document):
                op=row['op']
                actions=op.get('actions',[]) if op.get('kind')=='batch' else [op]
                if any(a.get('kind')=='object' and a.get('object')=='auto-upload-object' and a.get('x')==36 for a in actions):
                    return
            time.sleep(.1)
        self.fail('Transform commit remained local without an explicit sync command')

    def test_closing_observer_does_not_release_remote_selection_owner(self):
        owner=self.proxy('close-owner',self.url)
        observer=self.proxy('close-observer',self.url)
        self.call(owner,'object','put','-',value={'object':'owned-after-close','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(owner,'sync',self.url)
        self.call(observer,'sync',self.url)
        self.call(owner,'selection','select','-',actor='tablet-b',value={'session':'owned-session','page':0,'entities':[{'kind':'object','id':'owned-after-close'}]})
        self.call(observer,'sync',self.url)
        self.assertIn('0:object:owned-after-close',self.call(observer,'snapshot')['leases'])
        observer.stop()
        self.assertIn('0:object:owned-after-close',self.locks())
        owner.stop()
        self.assertNotIn('0:object:owned-after-close',self.locks())

    def test_offline_selection_can_commit_atomically(self):
        a=self.proxy('offline')
        self.call(a,'object','put','-',value={'object':'offline-object','shape':'rect','x':5,'y':5,'w':50,'h':40})
        self.call(a,'selection','select','-',value={'session':'offline-session','page':0,'entities':[{'kind':'object','id':'offline-object'}]})
        result=self.call(a,'transform','commit','-',value={'session':'offline-session','entities':[{'kind':'object','id':'offline-object','page':0}],'transform':{'dx':20,'scale':2}})
        self.assertTrue(result)
        changed=next(o for o in self.call(a,'inspect')['objects'] if o['object']=='offline-object')
        self.assertEqual((changed['x'],changed['w']),(0,100))
        self.assertEqual(self.call(a,'status')['pending'],2)

    def test_transform_rotation_then_flip_composes_about_pivot(self):
        a=self.proxy('rotate-flip')
        self.call(a,'object','put','-',value={'object':'turned-text','shape':'text','text':'turn me','x':40,'y':30,'w':80,'h':30,'rotation':0})
        entity={'kind':'object','id':'turned-text','page':0}
        self.call(a,'selection','select','-',value={'session':'turn-session','page':0,'entities':[entity]})
        self.call(a,'transform','commit','-',value={'session':'turn-session','entities':[entity],'transform':{'rotation':30,'pivotX':80,'pivotY':45}})
        self.call(a,'transform','commit','-',value={'session':'turn-session','entities':[entity],'transform':{'flipX':True,'pivotX':80,'pivotY':45}})
        changed=next(o for o in self.call(a,'inspect')['objects'] if o['object']=='turned-text')
        self.assertEqual(changed['rotation'],-30)
        self.assertTrue(changed['flipX'])
        self.assertEqual((changed['x'],changed['y'],changed['w'],changed['h']),(40,30,80,30))

    def test_locked_sources_block_fill_duplicate_and_page_move(self):
        a=self.proxy('source-guards')
        self.call(a,'layer','put','-',value={'layer':'guard-layer','name':'Guard Layer','force':False})
        self.call(a,'object','put','-',value={'object':'guard-source','layer':'guard-layer','shape':'rect','x':10,'y':10,'w':60,'h':60})
        self.call(a,'selection','select','-',actor='owner',value={'session':'guard-session','layer':'guard-layer','page':0,'entities':[{'kind':'object','id':'guard-source'}]})
        self.call(a,'fill','-',actor='other',value={'object':'guard-source','color':-65536},ok=False)
        self.call(a,'layer','duplicate','guard-layer',actor='other',ok=False)
        self.call(a,'object','update','guard-source',actor='other',value={'page':1},ok=False)
        self.assertEqual(self.call(a,'status')['pending'],2)

    def test_cli_copy_cut_paste_uses_local_clipboard(self):
        a=self.proxy('clipboard')
        self.call(a,'document','create','pages')
        self.call(a,'object','put','-',value={'object':'clipboard-object','shape':'text','text':'copy me','x':25,'y':35,'w':100,'h':30,'page':2})
        entity={'kind':'object','id':'clipboard-object','page':2}
        self.call(a,'selection','select','-',value={'session':'copy-session','page':2,'entities':[entity]})
        copied=self.call(a,'selection','copy','copy-session')
        self.assertEqual(copied['entities'][0]['text'],'copy me')
        clipboard=self.call(a,'snapshot')['clipboard']
        self.assertEqual(clipboard['entities'][0]['object'],'clipboard-object')
        self.assertGreater(clipboard['copiedAt'],0)
        self.call(a,'selection','deselect','copy-session')
        self.call(a,'selection','paste','-',value={'page':2,'layer':'base'})
        objects=self.call(a,'inspect')['objects'];pasted=next(o for o in objects if o['text']=='copy me' and o['object']!='clipboard-object')
        self.assertEqual((pasted['x'],pasted['y'],pasted['page']),(49,59,2))
        self.call(a,'selection','select','-',value={'session':'cut-session','page':2,'entities':[entity]})
        self.call(a,'selection','cut','cut-session')
        self.assertFalse(self.call(a,'inspect')['objectActive']['clipboard-object'])
        self.assertEqual(self.call(a,'snapshot')['clipboard']['entities'][0]['object'],'clipboard-object')


if __name__ == '__main__':
    unittest.main()

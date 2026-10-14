#!/usr/bin/env python3
"""Exercise causal edit conflicts through two offline CLI replicas."""
import tempfile
import uuid
from pathlib import Path
import json
import socket
import subprocess
import sys
import time
import urllib.request
import unittest

from server_checks import ROOT
from topology_support import Link, Proxy, eventually


class ConflictChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server_temp=tempfile.TemporaryDirectory(prefix='notemaker-conflict-server-')
        with socket.socket() as probe:
            probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
        cls.url=f'http://127.0.0.1:{port}'
        cls.process=subprocess.Popen([sys.executable,str(ROOT/'apps/server/server.py'),'--host','127.0.0.1','--port',str(port),'--database',str(Path(cls.server_temp.name)/'server.sqlite3')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for _ in range(100):
            try:urllib.request.urlopen(cls.url+'/api/events',timeout=1).close();break
            except OSError:time.sleep(.1)
        else:raise RuntimeError('NoteMaker server did not start')

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate();cls.process.wait(timeout=10);cls.server_temp.cleanup()

    def read(self,document):
        return json.load(urllib.request.urlopen(self.url+f'/api/events?document={document}',timeout=5))['events']

    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='notemaker-conflicts-')
        self.root=Path(self.temp.name)
        self.document='conflict-'+uuid.uuid4().hex
        self.link=Link(self.url,self.root/'edge.json')
        self.proxies=[]

    def tearDown(self):
        for proxy in self.proxies:proxy.stop()
        self.link.close()
        self.temp.cleanup()

    def proxy(self,name,directory=None):
        replica=Proxy(directory or self.root/name,self.document,upstream=self.link.target)
        self.proxies.append(replica)
        return replica

    @staticmethod
    def object_at(proxy,object_id='shared'):
        return next(obj for obj in proxy.cli('inspect')['objects'] if obj['object']==object_id)

    def common_pair(self,suffix='',page=0):
        a=self.proxy('replica-a'+suffix);b=self.proxy('replica-b'+suffix)
        if page>0:a.cli('document','create','pages',actor='seed')
        a.cli('object','put','-',actor='seed',value={
            'object':'shared','shape':'rect','page':page,'x':10,'y':20,'w':40,'h':30,
            'color':-16777216,'width':2
        })
        a.cli('sync',self.link.target)
        b.cli('sync',self.link.target)
        self.assertEqual(self.object_at(a),self.object_at(b))
        return a,b

    def branches(self,actor_a='shared-editor',actor_b='shared-editor',suffix='',page=0):
        a,b=self.common_pair(suffix,page)
        self.link.online=False
        a.cli('object','update','shared','-',actor=actor_a,value={'x':100})
        b.cli('object','update','shared','-',actor=actor_b,value={'x':200})
        # Both writes must remain durable while the common upstream is unavailable.
        self.assertEqual(self.object_at(a)['x'],100)
        self.assertEqual(self.object_at(b)['x'],200)
        self.link.online=True
        committed_before=len(self.read(self.document))
        self.link.duplicate=True
        a.cli('sync',self.link.target)
        b.cli('sync',self.link.target)
        self.link.duplicate=False
        a.cli('sync',self.link.target)
        self.assertEqual(len(self.read(self.document)),committed_before+2)
        return a,b

    def assert_converged(self,a,b):
        self.assertEqual(a.cli('inspect'),b.cli('inspect'))

    def test_same_actor_divergence_preserves_both_versions_and_resolution_history(self):
        for choice in ('local','remote','both'):
            with self.subTest(choice=choice):
                self.document='conflict-'+uuid.uuid4().hex+'-'+choice
                a,b=self.branches(suffix='-'+choice)
                self.assert_converged(a,b)
                conflicts=a.cli('conflicts')['conflicts']
                self.assertEqual(len(conflicts),1,conflicts)
                conflict=conflicts[0]
                self.assertEqual({conflict['local']['x'],conflict['remote']['x']},{100,200})
                self.assertTrue(conflict['localVersion'])
                self.assertTrue(conflict['remoteVersion'])
                # Both branch writes were submitted twice by the link; the server
                # accepted each operation ID once and replay produced one conflict.
                self.assertEqual(len(a.cli('conflicts')['conflicts']),1)
                self.assert_converged(a,b)

                conflict_id=conflict['id']
                before_resolution=self.object_at(a)['x']
                chosen_x=conflict[choice]['x'] if choice!='both' else None
                a.cli('conflict','resolve',conflict_id,choice,actor='resolver')
                self.assertEqual(a.cli('conflicts')['conflicts'],[])
                resolved=a.cli('inspect')
                if choice in ('local','remote'):self.assertEqual(self.object_at(a)['x'],chosen_x)
                else:self.assertEqual(sorted(obj['x'] for obj in resolved['objects']),sorted((conflict['local']['x'],conflict['remote']['x']+24)))
                a.cli('sync',self.link.target);b.cli('sync',self.link.target)
                self.assert_converged(a,b)

                a.cli('undo',actor='resolver')
                self.assertEqual(len(a.cli('conflicts')['conflicts']),1)
                self.assertEqual(self.object_at(a)['x'],before_resolution)
                a.cli('sync',self.link.target);b.cli('sync',self.link.target)
                self.assert_converged(a,b)
                a.cli('redo',actor='resolver')
                self.assertEqual(a.cli('conflicts')['conflicts'],[])
                if choice=='both':self.assertEqual(sorted(obj['x'] for obj in a.cli('inspect')['objects']),sorted((conflict['local']['x'],conflict['remote']['x']+24)))
                else:self.assertEqual(self.object_at(a)['x'],chosen_x)
                a.cli('sync',self.link.target);b.cli('sync',self.link.target)
                self.assert_converged(a,b)
                for proxy in (a,b):proxy.stop()
                self.proxies=[]

    def test_different_actor_divergence_and_sequential_edit(self):
        a,b=self.branches(actor_a='alice',actor_b='bob')
        self.assertEqual(len(a.cli('conflicts')['conflicts']),1)
        self.assert_converged(a,b)
        a.cli('conflict','resolve',a.cli('conflicts')['conflicts'][0]['id'],'remote',actor='resolver')
        a.cli('sync',self.link.target);b.cli('sync',self.link.target)
        self.assert_converged(a,b)

        sequential=self.proxy('sequential')
        sequential.cli('sync',self.link.target)
        a.cli('object','update','shared','-',actor='alice',value={'x':300})
        a.cli('sync',self.link.target);sequential.cli('sync',self.link.target)
        sequential.cli('object','update','shared','-',actor='bob',value={'y':90})
        sequential.cli('sync',self.link.target);a.cli('sync',self.link.target)
        eventually(lambda: len(a.cli('conflicts')['conflicts'])==0)
        self.assertEqual(self.object_at(a)['x'],300)
        self.assertEqual(self.object_at(a)['y'],90)
        self.assert_converged(a,sequential)

    def test_page_two_branch_multi_edit_survives_proxy_restart(self):
        a,b=self.common_pair(suffix='-page-two',page=2)
        a_dir=a.directory;b_dir=b.directory
        self.link.online=False
        a.cli('object','update','shared','-',actor='one-actor',value={'x':100})
        b.cli('object','update','shared','-',actor='one-actor',value={'x':200})
        a.cli('object','update','shared','-',actor='one-actor',value={'y':80})
        b.cli('object','update','shared','-',actor='one-actor',value={'y':90})
        for proxy in (a,b):proxy.stop()
        self.proxies=[]
        a=self.proxy('replica-a-restart',directory=a_dir);b=self.proxy('replica-b-restart',directory=b_dir)
        self.assertEqual(self.object_at(a)['y'],80)
        self.assertEqual(self.object_at(b)['x'],200)
        self.assertEqual(self.object_at(b)['y'],90)
        self.link.online=True
        a.cli('sync',self.link.target);b.cli('sync',self.link.target);a.cli('sync',self.link.target)
        self.assert_converged(a,b)
        conflicts=a.cli('conflicts')['conflicts'];self.assertEqual(len(conflicts),1,conflicts)
        self.assertEqual(conflicts[0]['page'],2)
        self.assertEqual({conflicts[0]['local']['x'],conflicts[0]['remote']['x']},{100,200})
        self.assertEqual({conflicts[0]['local']['y'],conflicts[0]['remote']['y']},{80,90},conflicts)

    def test_hide_vs_edit_conflict_preserves_activity_snapshots(self):
        for upload_order in ('hide-first','edit-first'):
            with self.subTest(upload_order=upload_order):
                self.document='conflict-'+uuid.uuid4().hex+'-'+upload_order
                a,b=self.common_pair(suffix='-'+upload_order,page=4)
                self.link.online=False
                a.cli('object','hide','shared',actor='same-actor')
                b.cli('object','update','shared','-',actor='same-actor',value={'x':240})
                self.link.online=True
                first,second=(a,b) if upload_order=='hide-first' else (b,a)
                first.cli('sync',self.link.target);second.cli('sync',self.link.target);first.cli('sync',self.link.target)
                self.assert_converged(a,b)
                conflicts=a.cli('conflicts')['conflicts'];self.assertEqual(len(conflicts),1,conflicts)
                conflict=conflicts[0]
                self.assertEqual(conflict['page'],4)
                self.assertNotEqual(conflict['localActive'],conflict['remoteActive'])
                snapshots={conflict['local']['x'],conflict['remote']['x']}
                self.assertEqual(snapshots,{10,240},conflict)
                a.cli('conflict','resolve',conflict['id'],'local',actor='resolver')
                a.cli('sync',self.link.target);b.cli('sync',self.link.target)
                self.assert_converged(a,b)
                chosen=conflict['local'];chosen_active=conflict['localActive']
                self.assertEqual(self.object_at(a)['x'],chosen['x'])
                self.assertEqual(a.cli('inspect')['objectActive']['shared'],chosen_active)
                for proxy in (a,b):proxy.stop()
                self.proxies=[]

    def test_same_actor_sequential_edits_do_not_conflict(self):
        a,b=self.common_pair(suffix='-sequential-same-actor')
        actor='same-session'
        a.cli('object','update','shared','-',actor=actor,value={'x':310})
        a.cli('sync',self.link.target);b.cli('sync',self.link.target)
        b.cli('object','update','shared','-',actor=actor,value={'y':95})
        b.cli('sync',self.link.target);a.cli('sync',self.link.target)
        self.assertEqual(a.cli('conflicts')['conflicts'],[])
        self.assertEqual(b.cli('conflicts')['conflicts'],[])
        self.assertEqual(self.object_at(a)['x'],310);self.assertEqual(self.object_at(a)['y'],95)
        self.assert_converged(a,b)

    def test_simultaneous_resolutions_create_a_new_reviewable_conflict(self):
        a,b=self.branches(suffix='-simultaneous-resolve')
        conflict=a.cli('conflicts')['conflicts'][0]
        self.link.online=False
        a.cli('conflict','resolve',conflict['id'],'local',actor='resolver-a')
        b.cli('conflict','resolve',conflict['id'],'remote',actor='resolver-b')
        self.assertEqual(a.cli('conflicts')['conflicts'],[])
        self.assertEqual(b.cli('conflicts')['conflicts'],[])
        self.link.online=True
        a.cli('sync',self.link.target);b.cli('sync',self.link.target);a.cli('sync',self.link.target)
        self.assert_converged(a,b)
        unresolved=a.cli('conflicts')['conflicts']
        self.assertEqual(len(unresolved),1,unresolved)
        self.assertEqual({unresolved[0]['local']['x'],unresolved[0]['remote']['x']},{100,200})


if __name__=='__main__':
    unittest.main()

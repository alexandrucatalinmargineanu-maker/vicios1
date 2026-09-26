#!/usr/bin/env python3
"""End-to-end tests using the actual compiled Rust executable and signed local repos."""
import fcntl, importlib.util, json, os, pathlib, subprocess, tempfile, unittest
BASE=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('vpk',BASE/'tools/vpk.py'); vpk=importlib.util.module_from_spec(spec); spec.loader.exec_module(vpk)
BIN=pathlib.Path(os.environ.get('VOS_TEST_BINARY',BASE/'vos/target/debug/vos')).resolve()
class Integration(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.base=pathlib.Path(self.tmp.name); self.root=self.base/'root'; self.root.mkdir(); self.repo=self.base/'repo'; self.key=self.base/'key.pem'
        pub=vpk.keygen(self.key); (self.root/'etc/vos').mkdir(parents=True)
        (self.root/'etc/vos/repos.json').write_text(json.dumps({'index_url':(self.repo/'index.json').as_uri(),'public_key':pub}))
        self.records=[]; self.serial=1
    def pkg(self,name,version='1.0.0',depends=None,files=None,essential=False):
        manifest={'name':name,'version':version,'description':name,'abi':vpk.ABI,'depends':depends or [],'essential':essential}
        tree=self.base/f'tree-{name}-{version}'; tree.mkdir()
        for rel,content in (files or {f'usr/share/{name}/test':version}).items():
            dest=tree/rel; dest.parent.mkdir(parents=True,exist_ok=True); dest.write_text(content)
        record=vpk.pack(manifest,tree,self.repo/'packages'/f'{name}-{version}.vpk')
        self.records=[p for p in self.records if p['manifest']['name']!=name]+[record]; self.publish()
        return record
    def publish(self,expires=None): vpk.publish(self.repo,self.key,self.records,self.serial,expires)
    def vos(self,*args,ok=True):
        p=subprocess.run([str(BIN),'--root',str(self.root),*args],capture_output=True,text=True)
        self.assertEqual(p.returncode==0,ok,p.stdout+p.stderr); return p
    def db(self): return json.loads((self.root/'var/lib/vos/installed.json').read_text())
    def test_dependencies_and_execution(self):
        self.pkg('libdemo'); self.pkg('app',depends=[{'name':'libdemo','version':'^1.0'}],files={'usr/bin/demo':'#!/bin/sh\nprintf "working\\n"\n'})
        self.vos('install','app'); self.assertEqual(set(self.db()['packages']),{'libdemo','app'})
        result=subprocess.run(['/bin/sh',str(self.root/'usr/bin/demo')],capture_output=True,text=True,check=True); self.assertEqual(result.stdout,'working\n')
        self.vos('doctor'); self.vos('remove','libdemo',ok=False); self.vos('remove','app','libdemo')
    def test_missing_dependency_no_writes(self):
        self.pkg('app',depends=[{'name':'absent','version':'*'}]); self.vos('install','app',ok=False); self.assertFalse((self.root/'usr').exists())
    def test_incompatible_dependency(self):
        self.pkg('libdemo','2.0.0'); self.pkg('app',depends=[{'name':'libdemo','version':'^1.0'}]); self.vos('install','app',ok=False)
    def test_update(self):
        self.pkg('app'); self.vos('install','app'); self.pkg('app','1.1.0'); self.serial=2; self.publish(); self.vos('update'); self.assertEqual(self.db()['packages']['app']['manifest']['version'],'1.1.0')
    def test_signature_tampering(self):
        self.pkg('app'); f=self.repo/'index.json'; f.write_bytes(f.read_bytes()+b' '); self.vos('install','app',ok=False)
    def test_payload_tampering(self):
        rec=self.pkg('app'); f=self.repo/'packages'/rec['file']; f.write_bytes(f.read_bytes()+b'bad'); self.vos('install','app',ok=False); self.assertFalse((self.root/'usr').exists())
    def test_unmanaged_collision(self):
        self.pkg('app'); p=self.root/'usr/share/app/test'; p.parent.mkdir(parents=True); p.write_text('mine'); self.vos('install','app',ok=False); self.assertEqual(p.read_text(),'mine')
    def test_symlink_parent_escape(self):
        self.pkg('app'); outside=self.base/'outside'; outside.mkdir(); (self.root/'usr').symlink_to(outside); self.vos('install','app',ok=False); self.assertEqual(list(outside.iterdir()),[])
    def test_config_preservation(self):
        self.pkg('app',files={'etc/app.conf':'default'}); self.vos('install','app'); (self.root/'etc/app.conf').write_text('my config'); self.pkg('app','1.1.0',files={'etc/app.conf':'new'}); self.vos('update',ok=False); self.assertEqual((self.root/'etc/app.conf').read_text(),'my config')
    def test_essential(self):
        self.pkg('base',essential=True); self.vos('install','base'); self.vos('remove','base',ok=False)
    def test_rollback_index(self):
        self.pkg('app'); self.serial=10; self.publish(); self.vos('install','app'); self.serial=9; self.publish(); self.vos('update',ok=False)
    def test_expired_index(self):
        self.pkg('app'); self.publish(expires=1); self.vos('install','app',ok=False)
    def test_target_power_protection(self):
        self.vos('reboot',ok=False); self.vos('shutdown',ok=False)
    def test_dry_run(self):
        self.pkg('app'); self.vos('--dry-run','install','app'); self.assertFalse((self.root/'usr').exists())
    def test_lock(self):
        self.pkg('app'); state=self.root/'var/lib/vos'; state.mkdir(parents=True)
        with (state/'lock').open('w') as f:
            fcntl.flock(f,fcntl.LOCK_EX); self.vos('install','app',ok=False)
    def test_recovery(self):
        self.pkg('app'); self.vos('install','app'); p=self.root/'usr/share/app/test'; state=self.root/'var/lib/vos/pending'; state.mkdir(); (state/'0').write_bytes(p.read_bytes()); p.write_text('interrupted')
        (state/'journal.json').write_text(json.dumps([{'path':'usr/share/app/test','mode':420,'link':None,'file':'0','existed':True}]))
        self.vos('recover'); self.assertEqual(p.read_text(),'1.0.0')
    def test_version_reads_os(self):
        (self.root/'etc/os-release').write_text('ID=vicios\nVERSION_ID="0.2.0"\n'); self.assertEqual(self.vos('version').stdout,'0.2.0\n')
    def test_cycle(self):
        self.pkg('a',depends=[{'name':'b','version':'*'}]); self.pkg('b',depends=[{'name':'a','version':'*'}]); self.vos('install','a'); self.assertEqual(len(self.db()['packages']),2)
if __name__=='__main__': unittest.main(verbosity=2)

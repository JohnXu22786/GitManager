//! Synthetic packaging validation; actual ELF/runtime checks belong to CI.
#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;
    use std::process::Command;

    const CHECKS: &str = r#"
import argparse, hashlib, importlib.util, json, os, pathlib, stat, sys, tarfile, tempfile, unittest
from unittest import mock
spec = importlib.util.spec_from_file_location('scope_package', sys.argv.pop(1))
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)

class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.profile_environment = {'CARGO_PROFILE_TEST_OPT_LEVEL':'1',
            'CARGO_PROFILE_TEST_DEBUG_ASSERTIONS':'true', 'CARGO_PROFILE_TEST_OVERFLOW_CHECKS':'true'}
        environment = mock.patch.dict(os.environ, self.profile_environment)
        environment.start()
        self.addCleanup(environment.stop)
        self.root = pathlib.Path(self.temp.name) / 'checkout'
        self.root.mkdir()
        (self.root / 'target/debug/deps').mkdir(parents=True)
        self.binary = self.root / 'target/debug/deps/product_scope_runtime_view_test-123abc'
        self.binary.write_bytes(b'\x7fELF\x02\x01' + b'\0' * 12 + b'\x3e\0' + b'fixture' * 20)
        self.binary.chmod(0o755)
        (self.root / 'LICENSE').write_text('Synthetic license fixture\n')
        self.log = self.root.parent / 'build.jsonl'
        self.artifact = {'reason':'compiler-artifact', 'manifest_path':str(self.root / 'Cargo.toml'),
            'target':{'kind':['test'], 'name':p.TARGET_NAME, 'src_path':str(self.root / 'tests' / (p.TARGET_NAME + '.rs'))},
            'profile':{'test':True, 'opt_level':'1', 'debug_assertions':True, 'overflow_checks':True, 'debuginfo':2},
            'features':[], 'executable':str(self.binary)}
        self.write_log()
        self.args = argparse.Namespace(repo_root=self.root, build_json=self.log,
            source_sha='1'*40, output_dir=self.root.parent / 'output')
        self.identity = {'repository':p.REPOSITORY, 'source_sha':'1'*40, 'source_tree':'2'*40,
            'cargo_lock_sha256':'3'*64, 'workflow_path':'.github/workflows/ci.yml', 'workflow_blob':'4'*40,
            'github':{'event':'push','ref':p.BRANCH,'run_id':'123','run_attempt':'1','job':'test'},
            'runner':{'image':'fixture','image_version':'fixture'}}
    def write_log(self, records=None):
        records = records if records is not None else [self.artifact, {'reason':'build-finished','success':True}]
        self.log.write_text(''.join(json.dumps(x)+'\n' for x in records))
    def commands(self, argv, **kwargs):
        if argv[0] == 'rustc': return 'rustc fixture\nhost: '+p.HOST+'\n'
        if argv[0] == 'cargo': return 'cargo fixture\n'
        if argv[0] == 'readelf':
            return 'ELF64 x86-64\n[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]\n'
        self.assertEqual(pathlib.Path(argv[0]).name, p.TARGET_NAME)
        self.assertEqual(argv[1:], ['--list','--format','terse'])
        self.assertEqual(list(pathlib.Path(kwargs['cwd']).iterdir()), [])
        self.assertNotIn('LD_LIBRARY_PATH', kwargs['env'])
        self.assertNotIn('CARGO_HOME', kwargs['env'])
        self.assertTrue(pathlib.Path(kwargs['env']['HOME']).is_dir())
        return 'future_rehearsals_preserve_selected_promises_without_activating_cohorts: test\nadmission_cache_reuses_only_fresh_exact_contexts: test\n'
    def package(self):
        with mock.patch.object(p, 'source_identity', return_value=self.identity), mock.patch.object(p, 'command', side_effect=self.commands):
            return p.package(self.args)
    def test_selection(self):
        self.assertEqual(p.select_artifact(self.log, self.root)['executable'], str(self.binary))
        for field, values in [('opt_level',['0','2','3',None]), ('debug_assertions',[False,None]), ('overflow_checks',[False,None]), ('test',[False,None])]:
            for value in values:
                saved = self.artifact['profile'].copy()
                if value is None: del self.artifact['profile'][field]
                else: self.artifact['profile'][field] = value
                self.write_log()
                with self.assertRaises(ValueError): p.select_artifact(self.log, self.root)
                self.artifact['profile'] = saved
        self.write_log()
        for key in self.profile_environment:
            for value in ['0',None]:
                with mock.patch.dict(os.environ):
                    if value is None: del os.environ[key]
                    else: os.environ[key] = value
                    with self.assertRaises(ValueError): p.select_artifact(self.log, self.root)
        for field, value in [('manifest_path',str(self.root.parent/'Cargo.toml')), ('profile',{'test':True,'opt_level':'3','debug_assertions':False}), ('executable',str(self.root.parent/'foreign'))]:
            saved = self.artifact[field]
            self.artifact[field] = value
            self.write_log()
            with self.assertRaises(ValueError): p.select_artifact(self.log, self.root)
            self.artifact[field] = saved
        for records in [[self.artifact,self.artifact,{'reason':'build-finished','success':True}], [self.artifact,{'reason':'build-finished','success':False}], [self.artifact]]:
            self.write_log(records)
            with self.assertRaises(ValueError): p.select_artifact(self.log, self.root)
        self.write_log()
        real = self.binary.with_name('real')
        self.binary.rename(real)
        self.binary.symlink_to(real)
        with self.assertRaises(ValueError): p.select_artifact(self.log, self.root)
    def test_package(self):
        archive = self.package()
        self.assertEqual(len(list(self.args.output_dir.iterdir())), 4)
        inventory = json.loads(next(self.args.output_dir.glob('*.members.json')).read_text())
        metadata_copy = next(self.args.output_dir.glob('*.metadata.json')).read_bytes()
        with tarfile.open(archive, 'r:gz') as tar:
            names = tar.getnames()
            self.assertEqual(len(names), len(set(names)))
            self.assertEqual(set(names), set(inventory['members']))
            for member in tar.getmembers():
                self.assertTrue(member.isfile())
                data = tar.extractfile(member).read()
                expected = inventory['members'][member.name]
                self.assertEqual(expected, {'sha256':hashlib.sha256(data).hexdigest(),'size_bytes':len(data),'mode':member.mode})
            binary = tar.getmember('bin/'+p.TARGET_NAME)
            self.assertEqual(binary.mode, 0o755)
            self.assertEqual(tar.extractfile(binary).read(), self.binary.read_bytes())
            self.assertEqual(tar.extractfile('metadata.json').read(), metadata_copy)
            metadata = json.loads(metadata_copy)
            self.assertEqual(metadata['source'], self.identity)
            self.assertEqual(metadata['profile'], self.artifact['profile'])
            self.assertEqual(metadata['profile_environment'], self.profile_environment)
            self.assertEqual(metadata['distribution'], 'unsigned-ci-test-harness')
            self.assertTrue(metadata['direct_list']['success'])
            self.assertEqual(set(metadata['archive_members']), set(names))
        self.assertEqual(inventory['archive']['size_bytes'], archive.stat().st_size)
        self.assertEqual(inventory['archive']['sha256'], hashlib.sha256(archive.read_bytes()).hexdigest())
        before = {x.name:x.read_bytes() for x in self.args.output_dir.iterdir()}
        with self.assertRaises(FileExistsError): self.package()
        self.assertEqual(before, {x.name:x.read_bytes() for x in self.args.output_dir.iterdir()})
    def test_portability_failure(self):
        with mock.patch.object(p, 'source_identity', return_value=self.identity), mock.patch.object(p, 'command', side_effect=ValueError('missing loader')):
            with self.assertRaises(ValueError): p.package(self.args)
        self.assertFalse(self.args.output_dir.exists())
        for failure in ['loader_path','list_error','binary_changed']:
            def fail(argv, **kwargs):
                if failure == 'loader_path' and '--dynamic' in argv:
                    return '(RUNPATH) Library runpath: [/private/build/tree]\n'
                if pathlib.Path(argv[0]).name == p.TARGET_NAME:
                    if failure == 'list_error': raise ValueError('loader could not start')
                    if failure == 'binary_changed': pathlib.Path(argv[0]).write_bytes(b'changed')
                return self.commands(argv, **kwargs)
            with mock.patch.object(p, 'source_identity', return_value=self.identity), mock.patch.object(p, 'command', side_effect=fail):
                with self.assertRaises(ValueError): p.package(self.args)
            self.assertFalse(self.args.output_dir.exists())
        with mock.patch.object(p, 'source_identity', return_value=self.identity), mock.patch.object(p, 'command', side_effect=lambda argv, **kw: 'no tests' if pathlib.Path(argv[0]).name == p.TARGET_NAME else self.commands(argv, **kw)):
            with self.assertRaises(ValueError): p.package(self.args)
        self.assertFalse(self.args.output_dir.exists())
    def test_source_identity(self):
        environment = {'GITHUB_REPOSITORY':p.REPOSITORY,'GITHUB_EVENT_NAME':'push','GITHUB_REF':p.BRANCH,'GITHUB_SHA':'1'*40,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_JOB':'test'}
        (self.root/'Cargo.lock').write_bytes(b'fixture lock')
        def git(argv, **kw):
            if argv[1:] == ['diff','--exit-code','HEAD']: return ''
            if argv[-1] == 'HEAD': return '1'*40+'\n'
            if argv[-1] == 'HEAD^{tree}': return '2'*40+'\n'
            return '4'*40+'\n'
        with mock.patch.dict(os.environ, environment, clear=True), mock.patch.object(p, 'command', side_effect=git):
            self.assertEqual(p.source_identity(self.root,'1'*40)['source_tree'], '2'*40)
            with self.assertRaises(ValueError): p.source_identity(self.root,'5'*40)
            with mock.patch.object(p, 'command', side_effect=lambda argv, **kw: '5'*40 if argv[-1] == 'HEAD' else git(argv, **kw)):
                with self.assertRaises(ValueError): p.source_identity(self.root,'1'*40)
            os.environ['GITHUB_EVENT_NAME'] = 'pull_request'
            with self.assertRaises(ValueError): p.source_identity(self.root,'1'*40)

unittest.main()
"#;

    fn check(name: &str) {
        let result = Command::new("python3")
            .args(["-B", "-c", CHECKS])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package_scope_test.py"))
            .arg(format!("PackageTests.{name}"))
            .output()
            .expect("Linux scope packaging checks require Python 3");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[test]
    fn scope_harness_selection_rejects_ambiguous_foreign_or_reprofiled_artifacts() {
        check("test_selection");
    }
    #[test]
    fn scope_harness_package_binds_exact_members_modes_and_source_without_overwrite() {
        check("test_package");
    }
    #[test]
    fn scope_harness_portability_failure_never_publishes_a_package() {
        check("test_portability_failure");
    }
    #[test]
    fn scope_harness_source_identity_requires_exact_push_checkout() {
        check("test_source_identity");
    }
}

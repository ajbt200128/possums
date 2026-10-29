"""Exercise the source builder's npm handoff offline, stopping at a stub npm."""
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PINNED_NODE = Path('/nix/store/cx0jd5mwhn7ihryyv0qp9d33f8s1iwb7-nodejs-24.13.0/bin/node')
DIAGNOSTIC = 'possums_pi_build_cache_input'


class PiSourceBuildTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        selected = os.environ.get('POSSUMS_NODE')
        cls.node = str(Path(selected).resolve()) if selected else str(
            PINNED_NODE if PINNED_NODE.exists() else Path(shutil.which('node') or '/missing-node'))
        try:
            version = subprocess.run([cls.node, '--version'], capture_output=True,
                                     timeout=10, check=False)
        except OSError:
            raise unittest.SkipTest('Node 24.13.0 is required') from None
        if version.stdout.strip() != b'v24.13.0':
            raise unittest.SkipTest('Node 24.13.0 is required')

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='possums-pi-source-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.scratch = self.root / 'scratch'
        self.scratch.mkdir()
        self.capture = self.root / 'npm-capture.json'
        self.acquisition = self.root / 'unexpected-acquisition'
        self.pi = self.root / 'pi'
        for package in ('pi-ai', 'pi-coding-agent', 'pi-agent-core'):
            directory = self.pi / 'node_modules/@earendil-works' / package
            directory.mkdir(parents=True)
            (directory / 'package.json').write_text(json.dumps({'name': package, 'version': '1.3.0'}))
        self.verifier = self.root / 'verifier'
        self.verifier.mkdir()
        for name in ('verifier.wasm', 'wasm_exec.cjs', 'GO-LICENSE'):
            (self.verifier / name).write_text('synthetic public fixture')
        (self.verifier / 'verifier-build-report.json').write_text('{"sources":{}}')
        self.native = self.root / 'native'
        self.native.mkdir()
        for name in ('possums-tor-native', 'tor-native-build-report.json'):
            (self.native / name).write_text('synthetic public fixture; never executed')
        self.cache = self.root / 'public cache'
        self.cache.mkdir()
        self.tool('mktemp', '#!/bin/sh\nexec /usr/bin/mktemp -d ' +
                  shlex.quote(str(self.scratch / 'build-XXXXXX')) + '\n')
        self.tool('npm', f'#!{sys.executable}\n' +
                  'import json, os, pathlib, sys\n' +
                  f'pathlib.Path({str(self.capture)!r}).write_text(json.dumps({{\n' +
                  '"args": sys.argv[1:], "env": dict(os.environ), "cwd": os.getcwd(),\n' +
                  '"userconfig": pathlib.Path(os.environ["npm_config_userconfig"]).read_text(),\n' +
                  '"globalconfig": pathlib.Path(os.environ["npm_config_globalconfig"]).read_text(),\n' +
                  '"home_entries": len(list(pathlib.Path(os.environ["HOME"]).iterdir())),\n' +
                  '"tmp_entries": len(list(pathlib.Path(os.environ["TMPDIR"]).iterdir()))\n' +
                  '}))\nsys.exit(73)\n')
        # Never allow a regression to run an acquisition helper or compiler.
        for name in ('sh', 'go', 'rustc', 'cargo', 'rustup', 'gcc', 'cc'):
            self.tool(name, '#!/bin/sh\nprintf blocked > ' + shlex.quote(str(self.acquisition)) + '\nexit 74\n')
        self.env = {
            'PATH': f'{self.bin}:/usr/bin:/bin', 'POSSUMS_NODE': self.node,
            'POSSUMS_PI_ROOT': str(self.pi), 'POSSUMS_VERIFIER_BUILD': str(self.verifier),
            'POSSUMS_TOR_NATIVE_BUILD': str(self.native),
            'HOME': str(self.root / 'personal-home'), 'TMPDIR': str(self.root / 'personal-tmp'),
            'npm_config_userconfig': 'personal-config-canary',
            'npm_config_globalconfig': 'personal-config-canary',
            'npm_config_registry': 'personal-registry-canary', 'NPM_TOKEN': 'private-token-canary',
            'NODE_OPTIONS': '',
        }

    def tool(self, name, source):
        target = self.bin / name
        target.write_text(source)
        target.chmod(0o755)

    def run_build(self, **inputs):
        result = subprocess.run(['/bin/sh', str(ROOT / 'clients/pi/build.sh')],
                                env={**self.env, **inputs}, capture_output=True,
                                text=True, timeout=20, check=False)
        self.assertFalse(self.acquisition.exists(), 'acquisition/compiler stub was invoked')
        return result

    def assert_handoff(self, inputs, provided=False, offline=False):
        result = self.run_build(**inputs)
        self.assertEqual(result.returncode, 73, result.stderr)
        captured = json.loads(self.capture.read_text())
        builds = list(self.scratch.iterdir())
        self.assertEqual(len(builds), 1)
        scratch = builds[0]
        expected_cache = self.cache if provided else scratch / 'cache'
        expected = ['ci', '--legacy-peer-deps', '--ignore-scripts', '--no-audit', '--no-fund',
                    '--registry=https://registry.npmjs.org', f'--cache={expected_cache}']
        if offline:
            expected.append('--offline')
        self.assertEqual(captured['args'], expected)
        env = captured['env']
        self.assertEqual(env['HOME'], str(scratch / 'home'))
        self.assertEqual(env['TMPDIR'], str(scratch / 'tmp'))
        self.assertEqual(env['npm_config_userconfig'], str(scratch / 'user.npmrc'))
        self.assertEqual(env['npm_config_globalconfig'], str(scratch / 'global.npmrc'))
        self.assertEqual(env['npm_config_logs_max'], '0')
        self.assertEqual(env['npm_config_update_notifier'], 'false')
        self.assertEqual(env['PATH'], f'{Path(self.node).parent}:{self.bin}:/usr/bin:/bin')
        self.assertNotIn('canary', json.dumps(env))
        self.assertNotIn('POSSUMS_PI_NPM_CACHE', env)
        self.assertNotIn('POSSUMS_PI_NPM_OFFLINE', env)
        for field in ('userconfig', 'globalconfig'):
            self.assertEqual(captured[field], '')
        for field in ('home_entries', 'tmp_entries'):
            self.assertEqual(captured[field], 0)
        self.assertFalse((Path(captured['cwd']) / 'node_modules').exists())
        self.assertFalse((scratch / 'package').exists())

    def test_default_cache_and_default_online(self):
        self.assert_handoff({})

    def test_provided_cache_and_explicit_online(self):
        self.assert_handoff({'POSSUMS_PI_NPM_CACHE': str(self.cache),
                             'POSSUMS_PI_NPM_OFFLINE': '0'}, provided=True)

    def test_provided_cache_and_offline(self):
        self.assert_handoff({'POSSUMS_PI_NPM_CACHE': str(self.cache),
                             'POSSUMS_PI_NPM_OFFLINE': '1'}, provided=True, offline=True)

    def test_default_cache_and_offline(self):
        self.assert_handoff({'POSSUMS_PI_NPM_OFFLINE': '1'}, offline=True)

    def test_invalid_inputs_fail_before_acquisition(self):
        link = self.root / 'hostile-path-canary-link'
        link.symlink_to(self.cache, target_is_directory=True)
        regular = self.root / 'hostile-path-canary-file'
        regular.touch()
        invalid = [
            {'POSSUMS_PI_NPM_CACHE': value}
            for value in ('', 'hostile-path-canary-relative', str(self.root / 'hostile-path-canary-missing'),
                          str(link), str(link) + '/', str(regular))
        ] + [{'POSSUMS_PI_NPM_OFFLINE': value} for value in ('', '2', 'true', '01', 'hostile-flag-canary')]
        # Missing supplied build inputs would require acquisition if validation were late.
        self.env.pop('POSSUMS_VERIFIER_BUILD')
        self.env.pop('POSSUMS_TOR_NATIVE_BUILD')
        for inputs in invalid:
            with self.subTest(inputs=inputs):
                result = self.run_build(**inputs)
                self.assertEqual(result.returncode, 1)
                self.assertIn(DIAGNOSTIC, result.stderr)
                self.assertIn('Use an existing absolute nonsymlink directory', result.stderr)
                self.assertNotIn('canary', result.stderr)
                self.assertNotIn(str(self.root), result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertFalse(self.capture.exists())
                self.assertEqual(list(self.scratch.iterdir()), [])


if __name__ == '__main__':
    unittest.main()

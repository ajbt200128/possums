#!/usr/bin/env node
// Source-only, offline qualification. Reuse an existing pinned node_modules;
// never install dependencies, build a shipped client, or touch account state.
import { createRequire } from 'node:module';
import { mkdir, mkdtemp, symlink } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../..', import.meta.url));
let stage = 'pinned_dependencies';
try {
  if (!process.argv[2]) throw new Error('existing dependency path required');
  const deps = resolve(process.argv[2]);
  const require = createRequire(join(deps, 'esbuild/package.json'));
  const esbuild = require('esbuild');
  const ts = require('typescript');
  if (process.version !== 'v24.13.0' || ts.version !== '5.9.3' || esbuild.version !== '0.25.10') {
    throw new Error('pinned toolchain required');
  }
  stage = 'scratch_setup';
  const parent = join(root, 'target/intake-review');
  await mkdir(parent, { recursive: true });
  const scratch = await mkdtemp(join(parent, 'clients-'));
  await symlink(deps, join(scratch, 'node_modules'));
  // Match the release check harness layout for its pinned SDK dependency probes.
  const output = join(scratch, 'checks/build');
  const packageSource = join(scratch, 'source/clients/pi');
  await mkdir(output, { recursive: true });
  await mkdir(packageSource, { recursive: true });
  await symlink(deps, join(packageSource, 'node_modules'));
  const home = join(scratch, 'home');
  await mkdir(home);

  stage = 'source_typecheck';
  for (const project of ['clients/pi', 'clients/pi/core']) {
    const config = ts.readConfigFile(join(root, project, 'tsconfig.json'), ts.sys.readFile);
    if (config.error) throw new Error('configuration unavailable');
    const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, join(root, project));
    const options = { ...parsed.options, noEmit: true, incremental: false, typeRoots: [join(deps, '@types')] };
    const host = ts.createCompilerHost(options);
    host.resolveModuleNames = (names, containing) => names.map(name => {
      const found = ts.resolveModuleName(name, containing, options, host).resolvedModule;
      if (found || name.startsWith('.')) return found;
      return ts.resolveModuleName(name, join(deps, '../pinned-resolver.ts'), options, host).resolvedModule;
    });
    const program = ts.createProgram(parsed.fileNames, options, host);
    if (parsed.errors.length || ts.getPreEmitDiagnostics(program).length) throw new Error('typecheck failed');
  }

  stage = 'synthetic_bundle';
  const bundle = join(output, 'fixture.mjs');
  await esbuild.build({
    entryPoints: [join(root, 'clients/pi/test-entry.ts')], outfile: bundle,
    bundle: true, platform: 'node', format: 'esm', target: 'node24',
    nodePaths: [deps], external: ['@earendil-works/*'],
    define: { __PHASE01_FIXTURE__: 'true' }, logLevel: 'silent',
  });
  for (const [name, fixture] of [['channel-node', false], ['fixture-node', true]]) {
    await esbuild.build({
      entryPoints: [join(root, 'clients/pi/core/transport.ts')], outfile: join(output, name + '.mjs'),
      bundle: true, platform: 'node', format: 'esm', target: 'node24', nodePaths: [deps],
      define: { __PHASE01_FIXTURE__: String(fixture) }, logLevel: 'silent',
    });
  }
  const tmp = join(scratch, 'tmp');
  await mkdir(tmp);
  const env = {
    HOME: home, TMPDIR: tmp, PATH: dirname(process.execPath) + ':/usr/bin:/bin',
    INTAKE_DIAGNOSTIC_BUILD: bundle, PHASE01_BUILD: output,
    PHASE02_CLIENT_BUILD: output, PHASE02_CLIENT_INSTALL: dirname(deps),
    PHASE02_TEST_BUILD: bundle, PHASE02_SCRATCH: join(scratch, 'pi-cases'), POSSUMS_PI_ROOT: dirname(deps),
  };
  for (const name of ['intake_diagnostics', 'phase01_client', 'phase02_client', 'phase01_stream_limits', 'tinfoil_forwarding_client', 'phase02_pi']) {
    stage = name;
    const result = spawnSync(process.execPath, [join(root, 'tests', name + '.mjs')], {
      cwd: root, timeout: 120_000, maxBuffer: 1024 * 1024, env, encoding: 'utf8',
    });
    if (result.status !== 0 || result.error) throw new Error('synthetic checks failed');
    // Child output can include hostile fixture assertions. Do not relay or persist it.
    console.log(`${name}: pass`);
  }
  console.log('intake_client_qualification: pass; pinned source typechecks and offline client/host regressions');
} catch {
  console.error(`INTAKE_CLIENT_QUALIFICATION_UNAVAILABLE stage=${stage} constraint=pinned_offline_acceptance. No client qualification established. Check existing pinned dependencies and synthetic source checks; no raw diagnostic output was retained.`);
  process.exitCode = 1;
}

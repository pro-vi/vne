import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import test from 'node:test';

const scripts = dirname(fileURLToPath(import.meta.url));
const commit = 'a'.repeat(40);
const version = `vne 0.1.0 (${commit})`;
const archiveName = 'vne-0.1.0-aarch64-apple-darwin';
const binaryMarker = '#!/bin/sh\n# release fixture executable\nexit 0\n';
const validationStages = ['node:regressions', 'npm:run check', 'npm:run build', 'npm:run test', 'npm:run test:rust', 'security:'];

function executable(path, source) {
  writeFileSync(path, source);
  chmodSync(path, 0o755);
}

function directory(t) {
  const root = mkdtempSync(join(tmpdir(), 'vne release tests '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

const commandDouble = `#!${process.execPath}\n` + String.raw`
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const root = process.env.RELEASE_TEST_ROOT;
const settings = JSON.parse(fs.readFileSync(path.join(root, 'settings.json'), 'utf8'));
const command = path.basename(process.argv[1]);
const args = process.argv.slice(2);
const event = command + ':' + args.join(' ');
const log = path.join(root, 'events.log');
const earlier = fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : '';
fs.appendFileSync(log, event + '\n');
if (settings.fail === event) process.exit(19);
if (command === 'uname') console.log('Darwin arm64');
else if (command === 'git') {
  if (args[0] === 'status') {
    const later = earlier.includes('git:status');
    if (later && settings.fail === 'final-status') process.exit(19);
    if (settings.dirty === (later ? 'final' : 'initial')) console.log(' M tracked-file');
  } else if (args[1] === 'HEAD') {
    const later = earlier.includes('git:rev-parse HEAD');
    console.log(settings.changedHead && later ? 'b'.repeat(40) : 'a'.repeat(40));
  } else console.log(settings.wrongTag ? 'b'.repeat(40) : 'a'.repeat(40));
} else if (command === 'npm' || command === 'cargo' || command === 'security') {
  if (command === 'cargo' && process.env.CARGO_BUILD_JOBS !== '1') process.exit(21);
  if (command === 'npm' && args[1] === 'test:rust' && process.env.RUST_TEST_THREADS !== '1') process.exit(21);
} else {
  if (settings.fail === command) {
    if (command === 'shasum') console.log('a'.repeat(64) + '  archive');
    process.exit(19);
  }
  if (command === 'shasum' && settings.interrupt) {
    fs.writeFileSync(path.join(root, 'ready'), String(process.pid));
    const timer = setInterval(() => {
      if (fs.existsSync(path.join(root, 'continue'))) {
        clearInterval(timer);
        process.exit(0);
      }
    }, 10);
  } else {
    const result = spawnSync(settings.real[command], args, { encoding: 'utf8' });
    process.stdout.write(result.stdout);
    process.stderr.write(result.stderr);
    if (result.status === 0 && command === 'tar' && args.includes('-xzf')) {
      const binary = path.resolve(args[args.indexOf('-C') + 1], 'vne-0.1.0-aarch64-apple-darwin', 'vne');
      fs.writeFileSync(path.join(root, 'extracted-binary'), binary);
      if (settings.brokenExtracted) fs.writeFileSync(binary, '#!/bin/sh\nexit 19\n');
    }
    process.exit(result.status);
  }
}
`;

function releaseFixture(t, options = {}) {
  const root = directory(t);
  for (const path of ['scripts', 'tools', 'src-tauri/target/release', 'src-tauri/target/release-dist']) {
    mkdirSync(join(root, path), { recursive: true });
  }
  for (const name of ['release.sh', 'validate.sh']) {
    copyFileSync(join(scripts, name), join(root, 'scripts', name));
  }
  writeFileSync(join(root, 'scripts/check-release.mjs'), `
import assert from 'node:assert/strict';
import { appendFileSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
const root = process.env.RELEASE_TEST_ROOT;
const [binary, ...rest] = process.argv.slice(2);
assert.deepEqual([resolve(binary), ...rest], [readFileSync(root + '/extracted-binary', 'utf8'), ${JSON.stringify(version)}]);
assert.equal(readFileSync(binary, 'utf8'), ${JSON.stringify(binaryMarker)}, 'extracted executable marker');
appendFileSync(root + '/events.log', 'node:extracted-check\\n');
`);
  executable(join(root, 'scripts/security-check.sh'), '#!/bin/sh\nexec security\n');
  writeFileSync(join(root, 'scripts/test-release.mjs'),
    "import { appendFileSync, readFileSync } from 'node:fs';\n" +
    "const root = process.env.RELEASE_TEST_ROOT;\n" +
    "appendFileSync(root + '/events.log', 'node:regressions\\n');\n" +
    "if (JSON.parse(readFileSync(root + '/settings.json', 'utf8')).fail === 'node:regressions') process.exit(19);\n");
  writeFileSync(join(root, 'src-tauri/Cargo.toml'), '[package]\nversion = "0.1.0"\n');
  writeFileSync(join(root, 'LICENSE'), 'synthetic license');
  executable(join(root, 'src-tauri/target/release/vne'), binaryMarker);
  const real = {};
  for (const command of ['tar', 'shasum']) {
    const located = spawnSync('/usr/bin/which', [command], { encoding: 'utf8' });
    assert.equal(located.status, 0, `${command} is required for release tests`);
    real[command] = located.stdout.trim();
  }
  for (const command of ['uname', 'git', 'npm', 'cargo', 'security', ...Object.keys(real)]) {
    executable(join(root, 'tools', command), commandDouble);
  }
  const settings = { real, ...options };
  writeFileSync(join(root, 'settings.json'), JSON.stringify(settings));
  const output = join(root, 'src-tauri/target/release-dist');
  const archive = join(output, `${archiveName}.tar.gz`);
  writeFileSync(archive, 'previous archive');
  mkdirSync(join(output, '.release.unrelated'));
  const env = { ...process.env, RELEASE_TEST_ROOT: root, PATH: `${join(root, 'tools')}:${process.env.PATH}` };
  // Node 24.14.0 skips nested --test files when its runner marker is inherited.
  delete env.NODE_TEST_CONTEXT;
  return { root, output, archive, settings, env };
}

function release(fixture) {
  return spawnSync('/bin/sh', [join(fixture.root, 'scripts/release.sh')], {
    cwd: fixture.root, env: fixture.env, encoding: 'utf8', timeout: 20_000,
  });
}

function assertRefused(fixture, result) {
  assert.notEqual(result.status, 0);
  assert(!result.stdout.includes('archive: '), result.stdout);
  assert(!result.stdout.includes('class Vne'), result.stdout);
  assert.equal(readFileSync(fixture.archive, 'utf8'), 'previous archive');
  assert.deepEqual(readdirSync(fixture.output).sort(), ['.release.unrelated', `${archiveName}.tar.gz`].sort());
}

test('release validates once, checks the extracted archive, and emits matching checksum', (t) => {
  const fixture = releaseFixture(t);
  const result = release(fixture);
  assert.equal(result.status, 0, result.stderr);
  const events = readFileSync(join(fixture.root, 'events.log'), 'utf8').split('\n');
  let previous = -1;
  for (const stage of validationStages) {
    assert.equal(events.filter((event) => event === stage).length, 1, stage);
    assert(events.indexOf(stage) > previous, stage);
    previous = events.indexOf(stage);
  }
  const build = events.findIndex((event) => event.startsWith('cargo:build'));
  assert(build > previous);
  assert.equal(events.filter((event) => event === 'node:extracted-check').length, 1);
  assert(events.indexOf('node:extracted-check') > build);
  const digest = createHash('sha256').update(readFileSync(fixture.archive)).digest('hex');
  assert(result.stdout.includes(`sha256:  ${digest}`));
  assert(result.stdout.includes(`sha256 "${digest}"`));
  assert.deepEqual(readdirSync(fixture.output).sort(), ['.release.unrelated', `${archiveName}.tar.gz`].sort());
});

for (const options of [{ dirty: 'initial' }, { wrongTag: true }]) {
  test(`release precondition refuses ${JSON.stringify(options)}`, (t) => {
    const fixture = releaseFixture(t, options);
    assertRefused(fixture, release(fixture));
    assert(!readFileSync(join(fixture.root, 'events.log'), 'utf8').includes('npm:'));
  });
}

test('nested Node regression failure stops validation before the release build', (t) => {
  const fixture = releaseFixture(t, { fail: 'node:regressions' });
  assertRefused(fixture, release(fixture));
  const events = readFileSync(join(fixture.root, 'events.log'), 'utf8');
  assert(events.includes('node:regressions'));
  for (const later of validationStages.slice(1)) assert(!events.includes(later), later);
  assert(!events.includes('cargo:build'));
  assert(!events.includes('tar:'));
});

for (const options of [
  { fail: 'cargo:build --release --locked --manifest-path src-tauri/Cargo.toml --bin vne --features custom-protocol' },
  { fail: 'shasum' },
  { changedHead: true }, { dirty: 'final' }, { fail: 'final-status' },
]) {
  test(`release preserves existing archive on ${JSON.stringify(options)}`, (t) => {
    const fixture = releaseFixture(t, options);
    assertRefused(fixture, release(fixture));
  });
}

test('release rejects changed extracted bytes while the build output stays intact', (t) => {
  const fixture = releaseFixture(t, { brokenExtracted: true });
  const result = release(fixture);
  assertRefused(fixture, result);
  assert.match(result.stderr, /extracted executable marker/);
  assert.equal(readFileSync(join(fixture.root, 'src-tauri/target/release/vne'), 'utf8'), binaryMarker);
});

for (const symbolic of [false, true]) {
  test(`release never moves its archive inside a ${symbolic ? 'symlinked ' : ''}directory destination`, (t) => {
    const fixture = releaseFixture(t);
    rmSync(fixture.archive);
    const destination = symbolic ? join(fixture.root, 'unrelated destination') : fixture.archive;
    mkdirSync(destination);
    writeFileSync(join(destination, 'keep'), 'existing contents');
    if (symbolic) symlinkSync(destination, fixture.archive);
    const result = release(fixture);
    assert.deepEqual(readdirSync(destination), ['keep']);
    assert.equal(readFileSync(join(destination, 'keep'), 'utf8'), 'existing contents');
    if (symbolic) {
      assert.equal(result.status, 0, result.stderr);
      assert(lstatSync(fixture.archive).isFile(), 'replace the link, not its target');
    } else {
      assert.notEqual(result.status, 0, result.stdout);
      assert(!result.stdout.includes('archive: '));
      assert(!result.stdout.includes('class Vne'));
    }
  });
}

test('release cleans owned files on interruption and retries beside unrelated residue', async (t) => {
  const fixture = releaseFixture(t, { interrupt: true });
  const child = spawn('/bin/sh', [join(fixture.root, 'scripts/release.sh')], { cwd: fixture.root, env: fixture.env });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (data) => { stdout += data; });
  child.stderr.on('data', (data) => { stderr += data; });
  const completed = new Promise((resolve) => child.on('close', (status, signal) => resolve({ status, signal })));
  try {
    const deadline = Date.now() + 10_000;
    while (!existsSync(join(fixture.root, 'ready'))) {
      assert(Date.now() < deadline, 'release did not reach checksum stage');
      await delay(10);
    }
    child.kill('SIGTERM');
    writeFileSync(join(fixture.root, 'continue'), '');
    const result = await completed;
    assertRefused(fixture, { ...result, stdout, stderr });
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
    writeFileSync(join(fixture.root, 'continue'), '');
    await completed;
  }
  writeFileSync(join(fixture.root, 'settings.json'), JSON.stringify({ ...fixture.settings, interrupt: false }));
  const retry = release(fixture);
  assert.equal(retry.status, 0, retry.stderr);
  assert(existsSync(join(fixture.output, '.release.unrelated')));
});

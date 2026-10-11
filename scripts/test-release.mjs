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
const validationStages = ['node:regressions', 'npm:run check', 'npm:run build', 'npm:run test', 'npm:run test:rust', 'security:'];

function executable(path, source) {
  writeFileSync(path, source);
  chmodSync(path, 0o755);
}

const fakeVne = `#!${process.execPath}\n` + String.raw`
const fs = require('node:fs');
const { spawnSync } = require('node:child_process');
const fault = process.env.FAKE_VNE_FAULT;
const [command, file] = process.argv.slice(2);
if (fault === 'hang') setInterval(() => {}, 1000);
else if (fault === 'large-output') process.stdout.write('x'.repeat(2 * 1024 * 1024));
else if (command === '--version') {
  process.stdout.write((process.env.FAKE_VERSION || 'vne 0.1.0 (' + 'a'.repeat(40) + ')') + '\n');
} else if (command === 'check') {
  if (fault === 'check-status') process.exit(2);
  if (fault === 'check-json') { process.stdout.write('{'); process.exit(0); }
  if (fault === 'check-empty') { process.stdout.write('{}'); process.exit(0); }
  const entries = fs.readFileSync(file, 'utf8').split('\n').flatMap((line, index) => {
    const match = line.match(/^(?:export )?([A-Z_]+)=(.*)$/);
    return match ? [{ key: match[1], lineNumber: index + 1, valueState: match[2] ? 'set' : 'empty', value: '[withheld]' }] : [];
  });
  if (fault === 'check-key') entries[0].key = 'WRONG';
  if (fault === 'check-line') entries[0].lineNumber = 90;
  if (fault === 'check-state') entries[0].valueState = 'empty';
  const result = { file: { entries }, extra: 'additional fields are permitted' };
  const payload = entries[0].key === 'RELEASE_TEST_KEY' ? 'release-synthetic-value' : 'release-ordinary-before';
  if (fault === 'check-leak') result.extra = payload;
  if (fault === 'check-stderr') process.stderr.write(payload);
  let output = JSON.stringify(result);
  const encoded = [...payload].map((c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0')).join('');
  if (fault === 'check-encoded-value') output = output.replace('additional fields are permitted', encoded);
  if (fault === 'check-encoded-key') output = output.replace('extra', encoded);
  process.stdout.write(output);
} else if (command === 'set') {
  if (fault === 'set-status') process.exit(2);
  const value = fs.readFileSync(0, 'utf8');
  const old = fs.readFileSync(file, 'utf8');
  let updated = old.replace(/(APP_LABEL=')[^']*'/, '$1' + value + "'");
  if (fault === 'set-no-write') updated = old;
  if (fault === 'set-comment') updated = updated.replace('# keep', '# changed');
  fs.writeFileSync(file, updated);
  process.stdout.write(JSON.stringify({
    disposition: fault === 'set-disposition' ? 'alreadyPresent' : 'updated',
    key: fault === 'set-key' ? 'WRONG' : 'APP_LABEL',
    path: fault === 'set-path' ? '/wrong' : fs.realpathSync(file),
  }));
} else if (command === 'run') {
  if (fault === 'run-no-child') process.exit(0);
  const env = { ...process.env };
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    const match = line.match(/^(?:export )?([A-Z_]+)=(.*)$/);
    if (match) env[match[1]] = match[2].startsWith("'") ? match[2].split("'")[1] : match[2];
  }
  if (fault === 'run-label') env.APP_LABEL = 'wrong';
  if (fault === 'run-secret') env.VNE_TEST_SECRET = 'wrong';
  const split = process.argv.indexOf('--');
  const result = spawnSync(process.argv[split + 1], process.argv.slice(split + 2), { env, encoding: 'utf8' });
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  process.exit(fault === 'run-status' ? 0 : result.status);
} else process.exit(2);
`;

function directory(t) {
  const root = mkdtempSync(join(tmpdir(), 'vne release tests '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function check(binary, env = {}, expected = version) {
  return spawnSync(process.execPath, [join(scripts, 'check-release.mjs'), binary, expected], {
    encoding: 'utf8', env: { ...process.env, ...env }, timeout: 15_000,
  });
}

test('packaged executable check accepts a complete workflow and extra JSON fields', (t) => {
  const binary = join(directory(t), 'vne');
  executable(binary, fakeVne);
  const result = check(binary);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, 'release-check: passed\n');
});

const brokenBehavior = [
  ['check-status', 'check exit status'], ['check-json', 'JSON'], ['check-empty', 'check must return entries'],
  ['check-key', 'check metadata'], ['check-line', 'check metadata'], ['check-state', 'check metadata'],
  ['check-leak', 'stdout exposed'], ['check-stderr', 'stderr exposed'],
  ['check-encoded-value', 'decoded JSON exposed'], ['check-encoded-key', 'decoded JSON exposed'],
  ['set-status', 'set exit status'], ['set-disposition', 'set disposition'],
  ['set-key', 'set key'], ['set-path', 'set path'],
  ['set-no-write', 'preserve surrounding bytes'], ['set-comment', 'preserve surrounding bytes'],
  ['run-no-child', 'run must start the child'], ['run-label', 'run exit status'],
  ['run-secret', 'run exit status'], ['run-status', 'run exit status'],
  ['large-output', 'ENOBUFS'], ['hang', 'ETIMEDOUT'],
];
for (const [fault, message] of brokenBehavior) {
  test(`packaged executable check rejects ${fault}`, (t) => {
    const binary = join(directory(t), 'vne');
    executable(binary, fakeVne);
    const result = check(binary, { FAKE_VNE_FAULT: fault });
    assert.notEqual(result.status, 0);
    assert(result.stderr.includes(message), result.stderr);
    assert(!result.stdout.includes('release-check: passed'));
  });
}

for (const identity of ['vne 0.1.0', `vne 0.1.0 (${'b'.repeat(40)})`, `vne 0.1.0 (${commit}-dirty)`, `${version} `]) {
  test(`packaged executable check rejects identity ${identity}`, (t) => {
    const binary = join(directory(t), 'vne');
    executable(binary, fakeVne);
    const result = check(binary, { FAKE_VERSION: identity });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /release binary identity/);
  });
}

test('packaged executable check rejects a missing executable', (t) => {
  const result = check(join(directory(t), 'missing'));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /ENOENT/);
});

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
if (command === 'uname') console.log(settings.platform || 'Darwin arm64');
else if (command === 'git') {
  if (args[0] === 'status') {
    const later = earlier.includes('git:status');
    if (settings.fail === (later ? 'final-status' : 'initial-status')) process.exit(19);
    if (settings.dirty === (later ? 'final' : 'initial')) console.log(' M tracked-file');
  } else if (args[1] === 'HEAD') {
    const later = earlier.includes('git:rev-parse HEAD');
    console.log(settings.changedHead && later ? 'b'.repeat(40) : 'a'.repeat(40));
  } else console.log(settings.wrongTag ? 'b'.repeat(40) : 'a'.repeat(40));
} else if (command === 'npm' || command === 'cargo' || command === 'security') {
  if (command === 'cargo' && process.env.CARGO_BUILD_JOBS !== '1') process.exit(21);
  if (command === 'npm' && args[1] === 'test:rust' && process.env.RUST_TEST_THREADS !== '1') process.exit(21);
} else {
  if (settings.fail === command || (command === 'tar' && settings.fail === (args.includes('-xzf') ? 'extract' : 'archive'))) {
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
    if (result.status === 0 && command === 'tar' && args.includes('-xzf') && settings.brokenExtracted) {
      const binary = path.join(args[args.indexOf('-C') + 1], 'vne-0.1.0-aarch64-apple-darwin', 'vne');
      fs.writeFileSync(binary, fs.readFileSync(binary, 'utf8').replace('const fault = process.env.FAKE_VNE_FAULT;', "const fault = 'run-no-child';"));
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
  for (const name of ['release.sh', 'validate.sh', 'check-release.mjs']) {
    copyFileSync(join(scripts, name), join(root, 'scripts', name));
  }
  executable(join(root, 'scripts/security-check.sh'), '#!/bin/sh\nexec security\n');
  writeFileSync(join(root, 'scripts/test-release.mjs'),
    "import { appendFileSync, readFileSync } from 'node:fs';\n" +
    "const root = process.env.RELEASE_TEST_ROOT;\n" +
    "appendFileSync(root + '/events.log', 'node:regressions\\n');\n" +
    "if (JSON.parse(readFileSync(root + '/settings.json', 'utf8')).fail === 'node:regressions') process.exit(19);\n");
  writeFileSync(join(root, 'src-tauri/Cargo.toml'), '[package]\nversion = "0.1.0"\n');
  writeFileSync(join(root, 'LICENSE'), 'synthetic license');
  executable(join(root, 'src-tauri/target/release/vne'), fakeVne);
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
  assert(events.findIndex((event) => event.startsWith('cargo:build')) > previous);
  const digest = createHash('sha256').update(readFileSync(fixture.archive)).digest('hex');
  assert(result.stdout.includes(`sha256:  ${digest}`));
  assert(result.stdout.includes(`sha256 "${digest}"`));
  assert(result.stdout.includes('release-check: passed'));
  assert.deepEqual(readdirSync(fixture.output).sort(), ['.release.unrelated', `${archiveName}.tar.gz`].sort());
});

for (const options of [{ platform: 'Linux x86_64' }, { dirty: 'initial' }, { wrongTag: true }, { fail: 'initial-status' }]) {
  test(`release precondition refuses ${JSON.stringify(options)}`, (t) => {
    const fixture = releaseFixture(t, options);
    assertRefused(fixture, release(fixture));
    assert(!readFileSync(join(fixture.root, 'events.log'), 'utf8').includes('npm:'));
  });
}

for (const [index, fail] of validationStages.entries()) {
  test(`validation failure stops release at ${fail}`, (t) => {
    const fixture = releaseFixture(t, { fail });
    assertRefused(fixture, release(fixture));
    const events = readFileSync(join(fixture.root, 'events.log'), 'utf8');
    assert(events.includes(fail));
    for (const later of validationStages.slice(index + 1)) assert(!events.includes(later), later);
    assert(!events.includes('cargo:build'));
    assert(!events.includes('tar:'));
  });
}

for (const options of [
  { fail: 'cargo:build --release --locked --manifest-path src-tauri/Cargo.toml --bin vne --features custom-protocol' },
  { fail: 'archive' }, { fail: 'extract' }, { fail: 'shasum' },
  { changedHead: true }, { dirty: 'final' }, { fail: 'final-status' },
]) {
  test(`release preserves existing archive on ${JSON.stringify(options)}`, (t) => {
    const fixture = releaseFixture(t, options);
    assertRefused(fixture, release(fixture));
  });
}

test('release rejects the extracted executable even when staging passes', (t) => {
  const fixture = releaseFixture(t, { brokenExtracted: true });
  const staging = check(join(fixture.root, 'src-tauri/target/release/vne'));
  assert.equal(staging.status, 0, staging.stderr);
  const result = release(fixture);
  assertRefused(fixture, result);
  assert.match(result.stderr, /run must start the child/);
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

const rubyHarness = String.raw`
require 'pathname'
class Formula
  def self.desc(*) end
  def self.homepage(*) end
  def self.url(*) end
  def self.sha256(*) end
  def self.license(*) end
  def self.depends_on(*) end
  def self.test(&block) define_method(:run_test, &block) end
  def bin; Pathname.new(ENV.fetch('FORMULA_TEST_BIN')); end
  def testpath; Pathname.new(Dir.pwd); end
  def assert_equal(expected, actual); raise 'assert_equal failed' unless expected == actual; end
  def assert_match(expected, actual); raise 'assert_match failed' unless actual.include?(expected); end
  def refute_includes(actual, value); raise 'payload exposed' if actual.include?(value); end
  def shell_output(command)
    output = IO.popen(command, &:read)
    raise 'command status failed' unless $?.exitstatus == 0
    output
  end
end
load ARGV.fetch(0)
Vne.new.run_test
`;

test('generated Homebrew test executes env assertions against the installed path', (t) => {
  const fixture = releaseFixture(t);
  const result = release(fixture);
  assert.equal(result.status, 0, result.stderr);
  const formula = result.stdout.slice(result.stdout.indexOf('require "json"'));
  const formulaPath = join(fixture.root, 'formula.rb');
  writeFileSync(formulaPath, formula);
  const bin = join(fixture.root, 'installed bin');
  mkdirSync(bin);
  executable(join(bin, 'vne'), fakeVne);
  for (const fault of ['', 'check-status', 'check-json', 'check-empty', 'check-key', 'check-state', 'check-leak', 'check-encoded-value', 'check-encoded-key']) {
    const tested = spawnSync('ruby', ['-e', rubyHarness, formulaPath], {
      cwd: fixture.root, encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, FORMULA_TEST_BIN: bin, FAKE_VNE_FAULT: fault },
    });
    if (fault === '') assert.equal(tested.status, 0, tested.stderr);
    else assert.notEqual(tested.status, 0, `generated formula accepted ${fault}`);
  }
});

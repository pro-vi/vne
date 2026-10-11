import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const [executable, expectedVersion, ...extra] = process.argv.slice(2);
assert(executable && expectedVersion && extra.length === 0,
  'usage: node scripts/check-release.mjs <executable> <expected-version-line>');
const binary = resolve(executable);
const directory = mkdtempSync(join(tmpdir(), 'vne-release-check-'));
const file = join(directory, '.env');
const before = 'release-ordinary-before';
const after = 'release-ordinary-after';
const secret = 'release-synthetic-secret';
const content = `# release fixture\nexport APP_LABEL='${before}' # keep\nVNE_TEST_SECRET=${secret}\nEMPTY=\n`;

function cleanup() {
  rmSync(directory, { recursive: true, force: true });
}

for (const [signal, code] of [['SIGHUP', 129], ['SIGINT', 130], ['SIGTERM', 143]]) {
  process.once(signal, () => {
    cleanup();
    process.exit(code);
  });
}

function run(args, input, expectedStatus = 0) {
  const result = spawnSync(binary, args, {
    cwd: directory,
    encoding: 'utf8',
    input,
    timeout: 10_000,
    killSignal: 'SIGKILL',
    maxBuffer: 1024 * 1024,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, `${args[0]} ended by signal`);
  assert.equal(result.status, expectedStatus, `${args[0]} exit status`);
  for (const payload of [before, after, secret]) {
    assert(!result.stdout.includes(payload), `${args[0]} stdout exposed a fixture value`);
    assert(!result.stderr.includes(payload), `${args[0]} stderr exposed a fixture value`);
  }
  assert.equal(result.stderr, '', `${args[0]} wrote unexpected stderr`);
  return result.stdout;
}

function parseReceipt(output) {
  const parsed = JSON.parse(output);
  assert(parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed), 'expected a JSON object');
  const strings = [];
  function visit(value) {
    if (typeof value === 'string') strings.push(value);
    else if (value !== null && typeof value === 'object') {
      for (const [key, child] of Object.entries(value)) {
        strings.push(key);
        visit(child);
      }
    }
  }
  visit(parsed);
  for (const payload of [before, after, secret]) {
    assert(strings.every((value) => !value.includes(payload)), 'decoded JSON exposed a fixture value');
  }
  return parsed;
}

try {
  assert.equal(run(['--version']), `${expectedVersion}\n`, 'release binary identity');
  writeFileSync(file, content, { mode: 0o600 });
  const checked = parseReceipt(run(['check', file, '--json']));
  assert(Array.isArray(checked.file?.entries), 'check must return entries');
  assert.deepEqual(checked.file.entries.map((entry) => ({
    key: entry.key, lineNumber: entry.lineNumber, valueState: entry.valueState,
  })), [
    { key: 'APP_LABEL', lineNumber: 2, valueState: 'set' },
    { key: 'VNE_TEST_SECRET', lineNumber: 3, valueState: 'set' },
    { key: 'EMPTY', lineNumber: 4, valueState: 'empty' },
  ], 'check metadata');

  const changed = parseReceipt(run(['set', file, 'APP_LABEL', '--stdin', '--json'], after));
  assert.equal(changed.disposition, 'updated', 'set disposition');
  assert.equal(changed.key, 'APP_LABEL', 'set key');
  assert.equal(changed.path, realpathSync(file), 'set path');
  assert.equal(readFileSync(file, 'utf8'), content.replace(before, after), 'set must preserve surrounding bytes');

  const child = `import assert from 'node:assert/strict';
assert.equal(process.env.APP_LABEL, ${JSON.stringify(after)});
assert.equal(process.env.VNE_TEST_SECRET, ${JSON.stringify(secret)});
assert.equal(process.env.EMPTY, '');
process.stdout.write('release child: passed\\n');`;
  assert.equal(run(['run', file, '--', process.execPath, '--input-type=module', '-e', child]),
    'release child: passed\n', 'run must start the child');
  assert.equal(run(['run', file, '--', process.execPath, '-e', 'process.exit(17)'], undefined, 17),
    '', 'run must preserve the child status without output');
  process.stdout.write('release-check: passed\n');
} finally {
  cleanup();
}

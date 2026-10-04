import { describe, expect, it } from 'vitest';
import { sampleProject } from './sample';
import {
  ADD_ROW_ID,
  buildRows,
  duplicateHint,
  findExampleToCopy,
  initialSelection,
  missingKeysFor,
  needCount,
  nextNeedingIndex,
  type Row
} from './rows';
import type { EnvFile, ProjectSnapshot } from './types';

function file(snapshot: ProjectSnapshot, name: string): EnvFile {
  const found = snapshot.files.find((candidate) => candidate.name === name);
  if (!found) {
    throw new Error(`sample has no ${name}`);
  }
  return found;
}

function row(rows: Row[], key: string): Row {
  const found = rows.find((candidate) => candidate.key === key);
  if (!found) {
    throw new Error(`no row for ${key}`);
  }
  return found;
}

describe('window rows', () => {
  it('lists entries, then missing keys, then the add row', () => {
    const snapshot = sampleProject();
    const rows = buildRows(snapshot, file(snapshot, '.env'));

    expect(rows.map((candidate) => candidate.kind)).toEqual([...Array(11).fill('entry'), 'missing', 'add']);
    expect(rows.at(-1)?.id).toBe(ADD_ROW_ID);
    expect(row(rows, 'STRIPE_SECRET_KEY')).toMatchObject({
      kind: 'missing',
      need: 'missing',
      note: { tone: 'warning', text: 'missing · in .env.example' }
    });
  });

  it('marks empty and placeholder values except in the example file', () => {
    const snapshot = sampleProject();
    const envRows = buildRows(snapshot, file(snapshot, '.env'));
    const exampleRows = buildRows(snapshot, file(snapshot, '.env.example'));

    expect(row(envRows, 'NEXTAUTH_SECRET')).toMatchObject({ need: 'placeholder', note: { text: 'placeholder' } });
    expect(row(envRows, 'STRIPE_WEBHOOK_SECRET')).toMatchObject({ need: 'empty', note: { text: 'empty' } });
    expect(needCount(envRows)).toBe(3);
    expect(row(exampleRows, 'DATABASE_URL').need).toBeNull();
    expect(needCount(exampleRows)).toBe(0);
  });

  it('drops a missing key that another loaded file already sets', () => {
    const snapshot = sampleProject();
    const local = file(snapshot, '.env.local');
    local.entries.push({ ...local.entries[1], id: 'STRIPE_SECRET_KEY@3', key: 'STRIPE_SECRET_KEY', lineNumber: 3 });

    expect(missingKeysFor(snapshot, file(snapshot, '.env'))).toEqual([]);
    expect(missingKeysFor(snapshot, local)).toEqual([]);
  });

  it('keeps a missing key that another file lists only as empty', () => {
    const snapshot = sampleProject();
    const local = file(snapshot, '.env.local');
    local.entries.push({ ...local.entries[1], id: 'STRIPE_SECRET_KEY@3', key: 'STRIPE_SECRET_KEY', lineNumber: 3, valueState: 'empty' });

    expect(missingKeysFor(snapshot, file(snapshot, '.env'))).toEqual(['STRIPE_SECRET_KEY']);
  });

  it('notes duplicates and values another layer overrides', () => {
    const snapshot = sampleProject();
    const envRows = buildRows(snapshot, file(snapshot, '.env'));
    const localRows = buildRows(snapshot, file(snapshot, '.env.local'));

    const duplicates = envRows.filter((candidate) => candidate.key === 'FEATURE_ENABLED');
    expect(duplicates.map((candidate) => candidate.note?.text)).toEqual(['! duplicate of 12', '! duplicate of 11']);
    expect(row(envRows, 'DATABASE_URL').note).toEqual({ tone: 'quiet', text: '↳ overridden by .env.local' });
    expect(row(localRows, 'DATABASE_URL').note).toBeNull();
  });

  it('finds the next row needing a value, wrapping to the top', () => {
    const snapshot = sampleProject();
    const rows = buildRows(snapshot, file(snapshot, '.env'));
    const placeholder = rows.findIndex((candidate) => candidate.key === 'NEXTAUTH_SECRET');
    const empty = rows.findIndex((candidate) => candidate.key === 'STRIPE_WEBHOOK_SECRET');
    const missing = rows.findIndex((candidate) => candidate.key === 'STRIPE_SECRET_KEY');

    expect(nextNeedingIndex(rows, placeholder)).toBe(empty);
    expect(nextNeedingIndex(rows, missing)).toBe(placeholder);
    expect(nextNeedingIndex(rows, 0)).toBe(placeholder);

    const onlyEmpty: ProjectSnapshot = {
      ...snapshot,
      comparison: snapshot.comparison && { ...snapshot.comparison, missingKeys: [] },
      files: snapshot.files.map((candidate) => ({
        ...candidate,
        entries: candidate.entries.map((entry) =>
          entry.key === 'STRIPE_WEBHOOK_SECRET' ? entry : { ...entry, valueState: 'set' as const }
        )
      }))
    };
    const onlyEmptyRows = buildRows(onlyEmpty, file(onlyEmpty, '.env'));
    const onlyIndex = onlyEmptyRows.findIndex((candidate) => candidate.key === 'STRIPE_WEBHOOK_SECRET');
    expect(needCount(onlyEmptyRows)).toBe(1);
    expect(nextNeedingIndex(onlyEmptyRows, onlyIndex)).toBeNull();
  });

  it('gives the terminal command that keeps a duplicated line', () => {
    const snapshot = sampleProject();
    const env = file(snapshot, '.env');
    const [first, second] = env.entries.filter((entry) => entry.key === 'FEATURE_ENABLED');

    expect(duplicateHint(snapshot.root, env, second)).toBe(
      'set twice · to keep this line, run vne rm .env FEATURE_ENABLED --line 11'
    );
    expect(duplicateHint(snapshot.root, env, first)).toContain('--line 12');
    expect(duplicateHint(snapshot.root, env, env.entries[0])).toBeNull();

    env.entries.push({ ...second, id: 'FEATURE_ENABLED@20', lineNumber: 20 });
    expect(duplicateHint(snapshot.root, env, second)).toBe(
      'set 3 times · remove the others in a terminal, starting with vne rm .env FEATURE_ENABLED --line 11'
    );
  });

  it('offers to start .env from the root example only when .env is absent', () => {
    const snapshot = sampleProject();
    expect(findExampleToCopy(snapshot)).toBeNull();

    const fresh = { ...snapshot, comparison: null, files: snapshot.files.filter((candidate) => candidate.name !== '.env') };
    expect(findExampleToCopy(fresh)?.name).toBe('.env.example');

    const nestedOnly = {
      ...fresh,
      files: fresh.files.map((candidate) =>
        candidate.name === '.env.example' ? { ...candidate, path: '/demo/project/config/.env.example' } : candidate
      )
    };
    expect(findExampleToCopy(nestedOnly)).toBeNull();
  });

  it('opens on the first row needing a value in the compared file', () => {
    const snapshot = sampleProject();
    expect(initialSelection(snapshot)).toEqual({ path: '/demo/project/.env', rowId: 'NEXTAUTH_SECRET@13' });

    const settled: ProjectSnapshot = {
      ...snapshot,
      comparison: null,
      files: snapshot.files.map((candidate) => ({
        ...candidate,
        entries: candidate.entries.map((entry) => ({ ...entry, valueState: 'set' as const }))
      }))
    };
    expect(initialSelection(settled)).toEqual({ path: '/demo/project/.env', rowId: 'DATABASE_URL@2' });
    expect(initialSelection({ ...snapshot, files: [] })).toBeNull();
  });
});

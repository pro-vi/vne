import { baseName, relativePath } from './paths';
import type { EnvEntry, EnvFile, ProjectSnapshot } from './types';

/** Same list as `EXAMPLE_ENV_FILE_NAMES` in src-tauri/src/lib.rs. */
export const EXAMPLE_ENV_FILE_NAMES = ['.env.example', '.env.sample', '.env.template', '.env.defaults'];

/** Why a row still needs a value typed into it. */
export type Need = 'empty' | 'placeholder' | 'missing';

export type NoteTone = 'danger' | 'warning' | 'quiet';

export type RowNote = { tone: NoteTone; text: string };

/** One line of the window: a parsed entry, a key the example lists but the
 * file lacks, or the trailing row that adds a key. */
export type Row =
  | { kind: 'entry'; id: string; key: string; entry: EnvEntry; need: Need | null; note: RowNote | null }
  | { kind: 'missing'; id: string; key: string; need: 'missing'; note: RowNote }
  | { kind: 'add'; id: string; key: ''; need: null; note: null };

export const ADD_ROW_ID = 'add';

export function isExampleFile(file: EnvFile): boolean {
  return EXAMPLE_ENV_FILE_NAMES.includes(file.name);
}

/** A template's empty and placeholder values are its purpose, not gaps. */
export function entryNeed(file: EnvFile, entry: EnvEntry): Need | null {
  if (isExampleFile(file) || entry.valueState === 'set') {
    return null;
  }
  return entry.valueState;
}

/** Keys the example lists that this file lacks, unless another loaded
 * non-example file already sets them. */
export function missingKeysFor(snapshot: ProjectSnapshot, file: EnvFile): string[] {
  const comparison = snapshot.comparison;
  if (!comparison || comparison.basePath !== file.path) {
    return [];
  }

  const setElsewhere = new Set(
    snapshot.files
      .filter((other) => other.path !== file.path && !isExampleFile(other))
      .flatMap((other) => other.entries.filter((entry) => entry.valueState === 'set').map((entry) => entry.key))
  );
  return comparison.missingKeys.filter((key) => !setElsewhere.has(key));
}

export function buildRows(snapshot: ProjectSnapshot, file: EnvFile): Row[] {
  const exampleName = snapshot.comparison ? baseName(snapshot.comparison.examplePath) : '';
  const entries: Row[] = file.entries.map((entry) => ({
    kind: 'entry',
    id: entry.id,
    key: entry.key,
    entry,
    need: entryNeed(file, entry),
    note: entryNote(snapshot, file, entry)
  }));
  const missing: Row[] = missingKeysFor(snapshot, file).map((key) => ({
    kind: 'missing',
    id: `missing:${key}`,
    key,
    need: 'missing',
    note: { tone: 'warning', text: `missing · in ${exampleName}` }
  }));
  return [...entries, ...missing, { kind: 'add', id: ADD_ROW_ID, key: '', need: null, note: null }];
}

function entryNote(snapshot: ProjectSnapshot, file: EnvFile, entry: EnvEntry): RowNote | null {
  const others = otherLines(file, entry);
  if (others.length > 0) {
    return { tone: 'danger', text: `! duplicate of ${others.join(', ')}` };
  }

  const need = entryNeed(file, entry);
  if (need) {
    return { tone: 'warning', text: need };
  }

  const override = snapshot.layerReport.overrides.find(
    (candidate) => candidate.key === entry.key && candidate.files.includes(file.name) && candidate.effectiveFile !== file.name
  );
  return override ? { tone: 'quiet', text: `↳ overridden by ${override.effectiveFile}` } : null;
}

function otherLines(file: EnvFile, entry: EnvEntry): number[] {
  return file.entries
    .filter((candidate) => candidate.key === entry.key && candidate.id !== entry.id)
    .map((candidate) => candidate.lineNumber);
}

export function needCount(rows: Row[]): number {
  return rows.filter((row) => row.need !== null).length;
}

/** The next row that needs a value after `from`, wrapping to the top;
 * null when no other row needs one. */
export function nextNeedingIndex(rows: Row[], from: number): number | null {
  for (let step = 1; step < rows.length; step += 1) {
    const index = (from + step) % rows.length;
    if (rows[index].need !== null) {
      return index;
    }
  }
  return null;
}

/** The terminal command that keeps this occurrence of a duplicated key;
 * the window itself never removes lines. */
export function duplicateHint(root: string, file: EnvFile, entry: EnvEntry): string | null {
  const others = otherLines(file, entry);
  if (others.length === 0) {
    return null;
  }

  const command = `vne rm ${relativePath(root, file.path)} ${entry.key} --line ${others[0]}`;
  return others.length === 1
    ? `set twice · to keep this line, run ${command}`
    : `set ${others.length + 1} times · remove the others in a terminal, starting with ${command}`;
}

/** The root example file to start `.env` from, when the project has an
 * example but no root `.env`. */
export function findExampleToCopy(snapshot: ProjectSnapshot): EnvFile | null {
  const rootFiles = snapshot.files.filter((file) => file.path === joinRoot(snapshot.root, file.name));
  if (rootFiles.some((file) => file.name === '.env')) {
    return null;
  }
  return EXAMPLE_ENV_FILE_NAMES.map((name) => rootFiles.find((file) => file.name === name)).find(Boolean) ?? null;
}

/** Where the window opens: the first row needing a value, preferring the
 * file the example is compared against; otherwise the first row of the
 * first file. */
export function initialSelection(snapshot: ProjectSnapshot): { path: string; rowId: string } | null {
  const candidates = [...snapshot.files].sort(
    (left, right) => Number(right.path === snapshot.comparison?.basePath) - Number(left.path === snapshot.comparison?.basePath)
  );
  for (const file of candidates) {
    const rows = buildRows(snapshot, file);
    const needing = rows.find((row) => row.need !== null);
    if (needing) {
      return { path: file.path, rowId: needing.id };
    }
  }

  const first = snapshot.files[0];
  return first ? { path: first.path, rowId: buildRows(snapshot, first)[0].id } : null;
}

function joinRoot(root: string, name: string): string {
  return `${root.replace(/[\\/]+$/, '')}/${name}`;
}

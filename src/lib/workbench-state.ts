import type { EnvEntry, EnvFile, ProjectSnapshot } from './types';

export type EntryRef = {
  root: string;
  filePath: string;
  entryId: string;
  key: string;
  lineNumber: number;
};

export type RevealedEntry = {
  target: EntryRef;
  value: string;
};

export type OperationKind =
  | 'load-project'
  | 'browse-project'
  | 'reveal-entry'
  | 'save-entry'
  | 'add-entry'
  | 'copy-entry'
  | 'create-file'
  | 'create-from-example';

export type PendingOperation = {
  generation: number;
  kind: OperationKind;
  target: EntryRef | null;
};

export function createEntryRef(root: string, file: EnvFile, entry: EnvEntry): EntryRef {
  return {
    root,
    filePath: file.path,
    entryId: entry.id,
    key: entry.key,
    lineNumber: entry.lineNumber
  };
}

export function sameEntryRef(left: EntryRef | null | undefined, right: EntryRef | null | undefined): boolean {
  return Boolean(
    left &&
      right &&
      left.root === right.root &&
      left.filePath === right.filePath &&
      left.entryId === right.entryId &&
      left.key === right.key &&
      left.lineNumber === right.lineNumber
  );
}

export function createPendingOperation(
  generation: number,
  kind: OperationKind,
  target: EntryRef | null = null
): PendingOperation {
  return { generation, kind, target };
}

export function isCurrentOperation(pending: PendingOperation | null, completed: PendingOperation): boolean {
  return Boolean(pending && pending.generation === completed.generation && pending.kind === completed.kind);
}

export function canApplyTargetedOperation(
  pending: PendingOperation | null,
  completed: PendingOperation,
  activeTarget: EntryRef | null
): boolean {
  return isCurrentOperation(pending, completed) && sameEntryRef(completed.target, activeTarget);
}

export function requiresProtectedInput(snapshot: ProjectSnapshot | null, key: string): boolean {
  const normalizedKey = key.trim();
  if (!snapshot || !normalizedKey) {
    return true;
  }

  const matches = snapshot.files.flatMap((file) => file.entries.filter((entry) => entry.key === normalizedKey));
  return matches.length === 0 || matches.some((entry) => entry.shape.sensitive || entry.shape.redactedByDefault);
}

export function inputTypeForKey(snapshot: ProjectSnapshot | null, key: string, explicitlyVisible = false): 'password' | 'text' {
  return explicitlyVisible || !requiresProtectedInput(snapshot, key) ? 'text' : 'password';
}

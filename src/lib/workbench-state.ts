import type { EnvEntry, EnvFile, EnvFinding, ProjectSnapshot } from './types';

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

export type EditDraft = {
  target: EntryRef;
  baseline: string;
  value: string;
  revision: number;
};

export type OperationKind = 'load-project' | 'browse-project' | 'reveal-entry' | 'save-entry' | 'add-entry';

export type PendingOperation = {
  generation: number;
  kind: OperationKind;
  target: EntryRef | null;
  draftRevision: number | null;
};

export type DuplicateSaveConfirmation = 'this-occurrence';

export type DuplicateSaveConfirmations = Record<string, DuplicateSaveConfirmation | undefined>;

export type ResolvedFindingTarget = {
  file: EnvFile;
  entry: EnvEntry;
  target: EntryRef;
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

export function entryRefKey(target: EntryRef): string {
  return `${target.root}\u0000${target.filePath}\u0000${target.entryId}`;
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

export function createEditDraft(target: EntryRef, baseline: string): EditDraft {
  return { target, baseline, value: baseline, revision: 0 };
}

export function updateEditDraft(draft: EditDraft, value: string): EditDraft {
  return { ...draft, value, revision: draft.revision + 1 };
}

export function draftForTarget(draft: EditDraft | null, target: EntryRef | null): EditDraft | null {
  return draft && sameEntryRef(draft.target, target) ? draft : null;
}

export function isDirtyDraft(draft: EditDraft | null): boolean {
  return Boolean(draft && draft.value !== draft.baseline);
}

export function revealedValueFor(revealed: RevealedEntry | null, target: EntryRef | null): string | null {
  return revealed && sameEntryRef(revealed.target, target) ? revealed.value : null;
}

export function createPendingOperation(
  generation: number,
  kind: OperationKind,
  target: EntryRef | null = null,
  draftRevision: number | null = null
): PendingOperation {
  return { generation, kind, target, draftRevision };
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

export function parseDuplicateSaveConfirmation(value: string): DuplicateSaveConfirmation | null {
  return value === 'this-occurrence' ? value : null;
}

export function setDuplicateSaveConfirmation(
  confirmations: DuplicateSaveConfirmations,
  target: EntryRef,
  confirmation: DuplicateSaveConfirmation | null
): DuplicateSaveConfirmations {
  const next = { ...confirmations };
  const key = entryRefKey(target);
  if (confirmation) {
    next[key] = confirmation;
  } else {
    delete next[key];
  }
  return next;
}

export function isDuplicateSaveConfirmed(confirmations: DuplicateSaveConfirmations, target: EntryRef | null): boolean {
  return Boolean(target && confirmations[entryRefKey(target)] === 'this-occurrence');
}

export function isReviewOpen(requested: boolean, snapshot: ProjectSnapshot | null): boolean {
  return Boolean(requested && snapshot && snapshot.findings.length > 0);
}

export function resolveFindingTarget(snapshot: ProjectSnapshot, finding: EnvFinding): ResolvedFindingTarget | null {
  if (!finding.key) {
    return null;
  }

  const candidateFiles = finding.filePath
    ? snapshot.files.filter((candidate) => candidate.path === finding.filePath)
    : snapshot.files;
  if (candidateFiles.length === 0) {
    return null;
  }

  for (const file of candidateFiles) {
    const entry = finding.entryId
      ? file.entries.find((candidate) => candidate.id === finding.entryId && candidate.key === finding.key)
      : finding.lineNumber !== null
        ? file.entries.find((candidate) => candidate.key === finding.key && candidate.lineNumber === finding.lineNumber)
        : file.entries.find((candidate) => candidate.key === finding.key);
    if (entry) {
      return { file, entry, target: createEntryRef(snapshot.root, file, entry) };
    }
  }

  return null;
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

export function fileHasProtectedEntries(file: EnvFile | null): boolean {
  return Boolean(file?.entries.some((entry) => entry.shape.redactedByDefault));
}

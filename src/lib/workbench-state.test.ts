import { describe, expect, it } from 'vitest';
import { sampleProject } from './sample';
import {
  canApplyTargetedOperation,
  createEditDraft,
  createEntryRef,
  createPendingOperation,
  draftForTarget,
  fileHasProtectedEntries,
  inputTypeForKey,
  isDirtyDraft,
  isDuplicateSaveConfirmed,
  isReviewOpen,
  parseDuplicateSaveConfirmation,
  resolveFindingTarget,
  sameEntryRef,
  setDuplicateSaveConfirmation,
  updateEditDraft
} from './workbench-state';

describe('workbench state', () => {
  it('keeps same-looking entry ids distinct across files', () => {
    const snapshot = sampleProject();
    const firstFile = snapshot.files[0];
    const secondFile = snapshot.files[2];
    const firstEntry = firstFile.entries[0];
    const secondEntry = { ...secondFile.entries[0], id: firstEntry.id, key: firstEntry.key, lineNumber: firstEntry.lineNumber };
    const first = createEntryRef(snapshot.root, firstFile, firstEntry);
    const second = createEntryRef(snapshot.root, secondFile, secondEntry);

    expect(sameEntryRef(first, second)).toBe(false);
  });

  it('accepts a targeted result only for the current generation and target', () => {
    const snapshot = sampleProject();
    const file = snapshot.files[0];
    const revealTarget = createEntryRef(snapshot.root, file, file.entries[2]);
    const otherTarget = createEntryRef(snapshot.root, file, file.entries[4]);
    const operation = createPendingOperation(4, 'reveal-entry', revealTarget);

    expect(canApplyTargetedOperation(operation, operation, revealTarget)).toBe(true);
    expect(canApplyTargetedOperation(operation, operation, otherTarget)).toBe(false);
    expect(canApplyTargetedOperation(createPendingOperation(5, 'reveal-entry', revealTarget), operation, revealTarget)).toBe(false);
  });

  it('tracks one atomic draft and increments its revision on edits', () => {
    const snapshot = sampleProject();
    const file = snapshot.files[0];
    const target = createEntryRef(snapshot.root, file, file.entries[4]);
    const otherTarget = createEntryRef(snapshot.root, file, file.entries[6]);
    const clean = createEditDraft(target, 'before');
    const dirty = updateEditDraft(clean, 'after');

    expect(isDirtyDraft(clean)).toBe(false);
    expect(isDirtyDraft(dirty)).toBe(true);
    expect(dirty.revision).toBe(1);
    expect(draftForTarget(dirty, target)).toEqual(dirty);
    expect(draftForTarget(dirty, otherTarget)).toBeNull();
  });

  it('confirms only the selected duplicate occurrence', () => {
    const snapshot = sampleProject();
    const file = snapshot.files[0];
    const first = createEntryRef(snapshot.root, file, file.entries[7]);
    const second = createEntryRef(snapshot.root, file, file.entries[8]);
    const confirmations = setDuplicateSaveConfirmation({}, first, parseDuplicateSaveConfirmation('this-occurrence'));

    expect(isDuplicateSaveConfirmed(confirmations, first)).toBe(true);
    expect(isDuplicateSaveConfirmed(confirmations, second)).toBe(false);
    expect(parseDuplicateSaveConfirmation('anything-else')).toBeNull();
  });

  it('uses Rust-produced shape metadata and protects unknown inputs', () => {
    const snapshot = sampleProject();

    expect(inputTypeForKey(snapshot, 'DATABASE_URL')).toBe('password');
    expect(inputTypeForKey(snapshot, 'REDIS_URL')).toBe('password');
    expect(inputTypeForKey(snapshot, 'OPENAI_API_KEY')).toBe('password');
    expect(inputTypeForKey(snapshot, 'PORT')).toBe('text');
    expect(inputTypeForKey(snapshot, 'NEXT_PUBLIC_SITE_URL')).toBe('text');
    expect(inputTypeForKey(snapshot, 'UNCLASSIFIED_SETTING')).toBe('password');
    expect(inputTypeForKey(snapshot, 'UNCLASSIFIED_SETTING', true)).toBe('text');
  });

  it('fails closed when matching files disagree about sensitivity', () => {
    const snapshot = sampleProject();
    const safePort = snapshot.files[0].entries.find((entry) => entry.key === 'PORT');
    if (!safePort) {
      throw new Error('sample project must include PORT');
    }
    snapshot.files[1].entries.push({
      ...safePort,
      id: 'PORT@99',
      lineNumber: 99,
      shape: { ...safePort.shape, sensitive: true, redactedByDefault: true }
    });

    expect(inputTypeForKey(snapshot, 'PORT')).toBe('password');
  });

  it('uses one exact finding resolver for eligibility and navigation', () => {
    const snapshot = sampleProject();
    const duplicateFinding = snapshot.findings.find((finding) => finding.actionKind === 'resolve-duplicate-key');
    if (!duplicateFinding) {
      throw new Error('sample project must include a duplicate-key finding');
    }
    const resolved = resolveFindingTarget(snapshot, duplicateFinding);

    expect(resolved?.file.path).toBe(duplicateFinding.filePath);
    expect(resolved?.entry.id).toBe(duplicateFinding.entryId);
    expect(resolveFindingTarget(snapshot, { ...duplicateFinding, filePath: '/not/a/current/file' })).toBeNull();
    expect(resolveFindingTarget(snapshot, { ...duplicateFinding, entryId: 'FEATURE_ENABLED@999' })).toBeNull();
    expect(resolveFindingTarget(snapshot, { ...duplicateFinding, entryId: null, lineNumber: 999 })).toBeNull();
  });

  it('opens review only when requested findings still exist', () => {
    const snapshot = sampleProject();

    expect(isReviewOpen(true, snapshot)).toBe(true);
    expect(isReviewOpen(false, snapshot)).toBe(false);
    expect(isReviewOpen(true, { ...snapshot, findings: [] })).toBe(false);
    expect(isReviewOpen(true, null)).toBe(false);
  });

  it('keeps raw preview unavailable for files with classified protected entries', () => {
    const snapshot = sampleProject();

    expect(fileHasProtectedEntries(snapshot.files[0])).toBe(true);
    expect(fileHasProtectedEntries(snapshot.files[3])).toBe(false);
  });
});

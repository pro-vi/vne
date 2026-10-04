import { describe, expect, it } from 'vitest';
import { sampleProject } from './sample';
import { canApplyTargetedOperation, createEntryRef, createPendingOperation, inputTypeForKey, sameEntryRef } from './workbench-state';

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
});

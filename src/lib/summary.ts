import type { EnvComparison, EnvEntry, EnvFile, KeyStatus } from './types';

export function isEntryValueHidden(entry: EnvEntry, revealed: boolean): boolean {
  return entry.shape.redactedByDefault && !revealed;
}

export function keyStatus(file: EnvFile, entry: EnvEntry, comparison: EnvComparison | null): KeyStatus {
  if (file.duplicateKeys.includes(entry.key)) {
    return 'duplicate';
  }

  if (!comparison) {
    return 'ok';
  }

  if (file.path === comparison.examplePath && comparison.missingKeys.includes(entry.key)) {
    return 'missing';
  }

  if (file.path === comparison.basePath && comparison.extraKeys.includes(entry.key)) {
    return 'extra';
  }

  return 'ok';
}

export function statusLabel(status: KeyStatus): string {
  switch (status) {
    case 'duplicate':
      return 'Duplicate';
    case 'extra':
      return 'Extra';
    case 'missing':
      return 'Missing in actual';
    default:
      return 'OK';
  }
}

import type { EnvComparison, EnvEntry, EnvFile, KeyStatus, ProjectSnapshot } from './types';

export function totalIssueCount(snapshot: ProjectSnapshot | null): number {
  if (!snapshot) {
    return 0;
  }

  const comparisonIssues = snapshot.comparison
    ? snapshot.comparison.missingKeys.length + snapshot.comparison.extraKeys.length + snapshot.comparison.duplicateKeys.length
    : 0;
  const fileDiagnostics = snapshot.files.reduce((total, file) => total + file.diagnostics.length, 0);

  return comparisonIssues + fileDiagnostics;
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

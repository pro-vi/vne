export interface ProjectSnapshot {
  root: string;
  files: EnvFile[];
  /** Discovered candidates that could NOT be inspected, with a typed
   * reason ('unreadable' | 'invalid-utf8' | 'oversize' | 'count-limit' |
   * 'scan-timeout' | 'not-regular-file') — a scan is never "complete" while
   * this is silently empty (VNE-SEC-010; harden F0022 sync). */
  incomplete: IncompleteEnvFile[];
  comparison: EnvComparison | null;
  layerReport: EnvLayerReport;
  frameworkProfiles: FrameworkEnvProfile[];
  findings: EnvFinding[];
}

export interface IncompleteEnvFile {
  name: string;
  reason: string;
}

export type EnvFileCreationDisposition = 'created' | 'alreadyExists';

/** The window never receives the copied value, only when it will be cleared. */
export interface CopyOutcome {
  key: string;
  clearsInSeconds: number | null;
}

/** Sent when a concealed copy's time is up; `cleared` is false when something
 * else was copied in the meantime and the clipboard was left alone. */
export interface ClipboardCleared {
  key: string;
  cleared: boolean;
}

export interface EnsureEnvFileOutcome {
  disposition: EnvFileCreationDisposition;
  path: string;
}

export type EnvFileGitStatus =
  | 'tracked'
  | 'untrackedIgnored'
  | 'untrackedNotIgnored'
  | 'outsideRepository'
  | 'unknown';

export interface EnvFile {
  path: string;
  name: string;
  discoveryReasons: string[];
  entries: EnvEntry[];
  diagnostics: string[];
  duplicateKeys: string[];
  gitStatus: EnvFileGitStatus;
  content: string;
}

export interface EnvEntry {
  id: string;
  key: string;
  value: string;
  displayValue: string;
  valueState: EnvValueState;
  lineNumber: number;
  exported: boolean;
  quote: string | null;
  comment: string | null;
  shape: KeyShape;
  diagnostics: string[];
}

export type EnvValueState = 'set' | 'empty' | 'placeholder';

export interface KeyShape {
  kind: string;
  label: string;
  confidence: 'low' | 'medium' | 'high' | string;
  redactedByDefault: boolean;
  sensitive: boolean;
  exposure: string | null;
  reasons: string[];
}

export interface EnvComparison {
  basePath: string;
  examplePath: string;
  missingKeys: string[];
  extraKeys: string[];
  sharedKeys: string[];
  duplicateKeys: string[];
}

export interface EnvLayerReport {
  orderedFiles: EnvLayerFile[];
  overrides: EnvLayerOverride[];
  placeholderKeys: string[];
}

export interface EnvLayerFile {
  path: string;
  name: string;
  layerKind: string;
  precedence: number;
}

export interface EnvLayerOverride {
  key: string;
  files: string[];
  effectiveFile: string;
  conflict: boolean;
  redacted: boolean;
  summary: string;
}

export interface FrameworkEnvProfile {
  framework: string;
  mode: string;
  evidence: string[];
  orderedFiles: FrameworkEnvFile[];
  missingFiles: string[];
  notes: string[];
}

export interface FrameworkEnvFile {
  path: string;
  name: string;
  layerKind: string;
  rank: number;
}

export interface EnvFinding {
  severity: 'info' | 'warning' | string;
  actionKind: string;
  title: string;
  detail: string;
  evidence: string[];
  mutationPreview: string | null;
  filePath: string | null;
  key: string | null;
  lineNumber: number | null;
  entryId: string | null;
}


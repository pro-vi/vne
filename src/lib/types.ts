export interface ProjectSnapshot {
  root: string;
  files: EnvFile[];
  comparison: EnvComparison | null;
  layerReport: EnvLayerReport;
  frameworkProfiles: FrameworkEnvProfile[];
  repairActions: EnvRepairAction[];
}

export interface EnvFile {
  path: string;
  name: string;
  discoveryReasons: string[];
  entries: EnvEntry[];
  diagnostics: string[];
  duplicateKeys: string[];
  content: string;
}

export interface EnvEntry {
  key: string;
  value: string;
  displayValue: string;
  lineNumber: number;
  exported: boolean;
  quote: string | null;
  shape: KeyShape;
  diagnostics: string[];
}

export interface KeyShape {
  kind: string;
  label: string;
  confidence: 'low' | 'medium' | 'high' | string;
  redactedByDefault: boolean;
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

export interface EnvRepairAction {
  severity: 'info' | 'warning' | string;
  actionKind: string;
  title: string;
  detail: string;
  filePath: string | null;
  key: string | null;
}

export type KeyStatus = 'ok' | 'missing' | 'extra' | 'duplicate';

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { sampleProject } from './sample';
import type { ClipboardCleared, CopyOutcome, EnsureEnvFileOutcome, ProjectSnapshot } from './types';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

export async function loadProject(path: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    const sampleRoot = !path || path === '.' ? '/demo/project' : path;
    return redactProject(sampleProject(sampleRoot));
  }

  // The backend loads its authorized root; the path argument is only used
  // by the browser-preview sample.
  void path;
  return invoke<ProjectSnapshot>('load_project');
}

export async function initialProjectPath(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<string | null>('initial_project_path');
}

export async function revealEnvValue(root: string, path: string, key: string, lineNumber: number): Promise<string> {
  if (!isTauriRuntime()) {
    await delay(180);
    const snapshot = sampleProject(root || '/demo/project');
    const file = snapshot.files.find((candidate) => candidate.path === path);
    const entry = file?.entries.find((candidate) => candidate.key === key && candidate.lineNumber === lineNumber);
    if (!entry) {
      throw new Error(`Key \`${key}\` at line ${lineNumber} was not found`);
    }
    return entry.value;
  }

  return invoke<string>('reveal_env_value', { path: relativeIdentifier(root, path), key, lineNumber });
}

export async function copyEnvValue(root: string, path: string, key: string, lineNumber: number): Promise<CopyOutcome> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Copying is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<CopyOutcome>('copy_env_value', { path: relativeIdentifier(root, path), key, lineNumber });
}

export async function onClipboardCleared(handler: (event: ClipboardCleared) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }

  return listen<ClipboardCleared>('clipboard-cleared', (event) => handler(event.payload));
}

export async function saveEnvValue(
  root: string,
  path: string,
  key: string,
  lineNumber: number,
  value: string
): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Saving is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<ProjectSnapshot>('save_env_value', { path: relativeIdentifier(root, path), key, lineNumber, value });
}

export async function addEnvKey(root: string, path: string, key: string, value: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Adding missing keys is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<ProjectSnapshot>('add_env_key', { path: relativeIdentifier(root, path), key, value });
}

export async function ensureEnvFile(root: string, name: string): Promise<EnsureEnvFileOutcome> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Creating env files is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  const outcome = await invoke<unknown>('ensure_env_file', { name });
  return parseEnsureEnvFileOutcome(outcome);
}

export async function createEnvFromExample(): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Creating .env is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<ProjectSnapshot>('create_env_from_example');
}

export async function pickProjectDirectory(defaultPath: string): Promise<string | null> {
  void defaultPath;
  if (!isTauriRuntime()) {
    return null;
  }

  // Backend-owned dialog: the selected folder lands in Rust authorized-root
  // state directly; the webview only receives it for display.
  return invoke<string | null>('choose_authorized_root');
}

function relativeIdentifier(root: string, path: string): string {
  const normalizedRoot = root.endsWith('/') ? root : `${root}/`;
  if (path.startsWith(normalizedRoot)) {
    return path.slice(normalizedRoot.length);
  }
  return path.replace(/^\\/g, '/').split('/').pop() ?? path;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

export function parseEnsureEnvFileOutcome(value: unknown): EnsureEnvFileOutcome {
  if (!value || typeof value !== 'object' || !('disposition' in value) || !('path' in value)) {
    throw new Error('The native create-file response was malformed.');
  }

  const { disposition, path } = value;
  if ((disposition !== 'created' && disposition !== 'alreadyExists') || typeof path !== 'string' || !path) {
    throw new Error('The native create-file response was malformed.');
  }

  return { disposition, path };
}

function redactProject(snapshot: ProjectSnapshot): ProjectSnapshot {
  return {
    ...snapshot,
    files: snapshot.files.map((file) => {
      let content = file.content;
      const entries = file.entries.map((entry) => {
        if (!entry.shape.redactedByDefault) {
          return entry;
        }

        if (entry.value) {
          content = content.split(entry.value).join('********');
        }

        return { ...entry, value: '' };
      });

      return { ...file, content, entries };
    })
  };
}

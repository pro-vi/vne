import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { sampleProject } from './sample';
import type { ProjectSnapshot } from './types';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

export async function loadProject(path: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    const sampleRoot = !path || path === '.' ? '/demo/project' : path;
    return redactProject(sampleProject(sampleRoot));
  }

  return invoke<ProjectSnapshot>('load_project', { path });
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

  return invoke<string>('reveal_env_value', { root, path, key, lineNumber });
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

  return invoke<ProjectSnapshot>('save_env_value', { root, path, key, lineNumber, value });
}

export async function addEnvKey(root: string, path: string, key: string, value: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Adding missing keys is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<ProjectSnapshot>('add_env_key', { root, path, key, value });
}

export async function pickProjectDirectory(defaultPath: string): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  const selected = await open({
    title: 'Open project folder',
    directory: true,
    multiple: false,
    defaultPath: defaultPath || undefined
  });

  return Array.isArray(selected) ? (selected[0] ?? null) : selected;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
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

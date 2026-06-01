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
    return sampleProject(path || '/demo/project');
  }

  return invoke<ProjectSnapshot>('load_project', { path });
}

export async function revealProject(path: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    return sampleProject(path || '/demo/project');
  }

  return invoke<ProjectSnapshot>('reveal_project', { path });
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

export async function addEnvKey(root: string, path: string, key: string): Promise<ProjectSnapshot> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Adding missing keys is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<ProjectSnapshot>('add_env_key', { root, path, key });
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

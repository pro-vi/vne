import { invoke } from '@tauri-apps/api/core';
import { sampleProject } from './sample';
import type { EnvFile, ProjectSnapshot } from './types';

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

export async function saveEnvValue(path: string, key: string, value: string): Promise<EnvFile> {
  if (!isTauriRuntime()) {
    await delay(180);
    throw new Error('Saving is available in the Tauri desktop app. Browser preview uses read-only sample data.');
  }

  return invoke<EnvFile>('save_env_value', { path, key, value });
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

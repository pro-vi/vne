import { describe, expect, it } from 'vitest';
import viteConfig from '../../vite.config';

describe('vite config', () => {
  it('builds Tauri assets with relative paths', () => {
    expect(viteConfig).toMatchObject({ base: './' });
  });
});

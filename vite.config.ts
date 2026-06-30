import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  base: './',
  clearScreen: false,
  server: {
    host: host ?? '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  }
});

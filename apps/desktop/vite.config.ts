import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // The licence notices (about 525 kB) load on their own, only in Settings ›
  // Licences; the app's own code stays far below this.
  build: { chunkSizeWarningLimit: 600 },
  // tokens.test.ts reads the design tokens as text.
  test: { css: { include: [/tokens\.css/] } },
});

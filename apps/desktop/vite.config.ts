import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // tokens.test.ts reads the design tokens as text.
  test: { css: { include: [/tokens\.css/] } },
});

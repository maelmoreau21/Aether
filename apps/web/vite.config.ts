import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';
export default defineConfig({
  plugins: [sveltekit({ adapter: adapter(), paths: { base: '/app', relative: false }, csp: { mode: 'hash', directives: { 'script-src': ['self', 'wasm-unsafe-eval'] } } })],
  // Fonts stay same-origin files so the Core CSP can keep `font-src 'self'` without `data:`.
  build: { assetsInlineLimit: (file) => (/\.(woff2?|ttf|otf)$/i.test(file) ? false : undefined) },
  test: { include: ['src/**/*.test.ts'], environment: 'node' }
});

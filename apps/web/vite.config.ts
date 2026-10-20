import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';
export default defineConfig({ plugins: [sveltekit({ adapter: adapter(), paths: { base: '/app' }, csp: { mode: 'hash', directives: { 'script-src': ['self'] } } })], test: { include: ['src/**/*.test.ts'], environment: 'node' } });

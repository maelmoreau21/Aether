// Keep Axum's strict script-src 'self' without inline execution or unsafe-inline.
import { readFileSync, writeFileSync } from 'node:fs';
const file = new URL('../build/index.html', import.meta.url);
let html = readFileSync(file, 'utf8');
let count = 0;
html = html.replace(/<script>([\s\S]*?)<\/script>/g, (_, body) => {
  const name = `bootstrap-${count++}.js`;
  writeFileSync(new URL(`../build/${name}`, import.meta.url), body);
  return `<script src="./${name}"></script>`;
});
if (count !== 1) throw new Error(`Expected one SvelteKit bootstrap, got ${count}`);
writeFileSync(file, html);

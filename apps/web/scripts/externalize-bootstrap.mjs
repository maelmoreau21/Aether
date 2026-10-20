// Keep Axum's strict script-src 'self' without inline execution or unsafe-inline.
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../build/', import.meta.url));
for (const entry of readdirSync(root, { recursive: true, withFileTypes: true })) {
  if (!entry.isFile() || !entry.name.endsWith('.html')) continue;
  const file = join(entry.parentPath, entry.name);
  let count = 0;
  const html = readFileSync(file, 'utf8').replace(/<script>([\s\S]*?)<\/script>/g, (_, body) => {
    const name = `${entry.name.replace('.html', '')}-bootstrap-${count++}.js`;
    writeFileSync(join(dirname(file), name), body);
    return `<script src="./${name}"></script>`;
  });
  if (count !== 1) throw new Error(`Expected one SvelteKit bootstrap in ${file}, got ${count}`);
  writeFileSync(file, html);
}

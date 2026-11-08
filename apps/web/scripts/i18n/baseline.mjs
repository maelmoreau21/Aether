// Rewrites src/lib/i18n/hardcoded-baseline.json: the number of sentences still written directly in each
// source file. The guard test fails when a file exceeds its entry, so the number can only go down.
// Run `npm run i18n:baseline` after translating a file and commit the smaller baseline.
import fs from 'node:fs';
import path from 'node:path';
import { IGNORED, countLiterals } from './scan.mjs';

const root = path.resolve(import.meta.dirname, '../../src');
const counts = {};
(function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) { walk(file); continue; }
    if (!/\.(svelte|ts)$/.test(entry.name)) continue;
    const posix = file.split(path.sep).join('/');
    if (IGNORED.some(pattern => pattern.test(posix))) continue;
    const count = countLiterals(fs.readFileSync(file, 'utf8'), posix);
    if (count > 0) counts[path.relative(root, file).split(path.sep).join('/')] = count;
  }
})(root);
const sorted = Object.fromEntries(Object.entries(counts).sort(([a], [b]) => a.localeCompare(b)));
fs.writeFileSync(path.join(root, 'lib/i18n/hardcoded-baseline.json'), `${JSON.stringify(sorted, null, 2)}\n`);
console.log(`${Object.keys(sorted).length} files, ${Object.values(sorted).reduce((a, b) => a + b, 0)} sentences remain`);

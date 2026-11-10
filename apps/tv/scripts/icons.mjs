// Renders the TV launcher icons from the brand mark (apps/tv/shell/logo.svg) with the Chromium of
// apps/web's Playwright: `node apps/tv/scripts/icons.mjs` after `npm ci` in apps/web.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const require = createRequire(new URL('../../web/package.json', import.meta.url));
const { chromium } = require('playwright');
const here = path => fileURLToPath(new URL(path, import.meta.url));
const logo = readFileSync(here('../shell/logo.svg'), 'utf8');

// Tizen TV: 512 × 423; webOS: 80 × 80 and 130 × 130. Halo canvas, mark at 62 % of the short side.
const icons = [
  { file: '../tizen/icon.png', width: 512, height: 423 },
  { file: '../webos/icon.png', width: 80, height: 80 },
  { file: '../webos/largeIcon.png', width: 130, height: 130 }
];

const browser = await chromium.launch({ headless: true });
try {
  for (const icon of icons) {
    const page = await browser.newPage({ viewport: { width: icon.width, height: icon.height } });
    const size = Math.round(Math.min(icon.width, icon.height) * 0.62);
    await page.setContent(`<!doctype html><style>html,body{margin:0;height:100%;background:#0b0e18;display:grid;place-items:center}svg{width:${size}px;height:${size}px}</style>${logo}`);
    await page.screenshot({ path: here(icon.file), omitBackground: false });
    await page.close();
    console.log(`${icon.file} ${icon.width}×${icon.height}`);
  }
} finally {
  await browser.close();
}

// Browser interaction with synthetic authenticated catalogue APIs; no remote provider involved.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5193/app/';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/people';
fs.mkdirSync(out, { recursive: true });
const portrait = '/api/v1/works/film-0/people/0123456789abcdef0123456789abcdef/image';
const work = (id, kind = 'film') => ({ id, kind, title: `Œuvre ${id}`, year: 2026, description: '', libraryId: 'film', synthetic: false, actors: [{ name: 'Camille Test', role: 'Personnage', imageUrl: portrait }], directors: ['Réalisatrice Exemple'], crewCredits: [{ name: 'Réalisatrice Exemple', role: 'Director', imageUrl: portrait }], artwork: {} });
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await fixturePage(browser, width, 'dark');
      let calls = 0;
      await page.route('**/api/v1/catalog**', async route => {
        const url = new URL(route.request().url());
        if (url.pathname === '/api/v1/catalog/film-0') return route.fulfill({ json: work('film-0') });
        if (url.pathname === '/api/v1/catalog' && url.searchParams.has('person')) {
          assert.equal(url.searchParams.get('person'), 'Camille Test');
          calls += 1;
          const offset = Number(url.searchParams.get('offset'));
          return route.fulfill({ json: { items: offset ? [work('film-2', 'series')] : [work('film-1')], offset, limit: 24, hasMore: offset === 0 } });
        }
        return route.fallback();
      });
      await page.route('**/api/v1/works/film-0/people/**/image', route => route.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="300"><rect width="200" height="300" fill="red"/></svg>' }));
      await page.goto(base + 'films/?work=film-0');
      const cast = page.getByRole('link', { name: 'Voir les œuvres de Camille Test' });
      await cast.waitFor();
      await cast.focus();
      await page.keyboard.press('Enter');
      await page.getByRole('heading', { name: 'Camille Test' }).waitFor();
      await page.getByRole('link', { name: 'Voir Œuvre film-1' }).waitFor();
      await page.locator('.person-header .portrait img').waitFor();
      assert.equal(await page.locator('.person-header img').getAttribute('src'), portrait);
      await page.getByRole('button', { name: 'Afficher plus' }).click();
      await page.getByRole('link', { name: 'Voir Œuvre film-2' }).waitFor();
      assert.equal(calls, 2);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      assert.deepEqual(errors, []);
      await page.screenshot({ path: `${out}/people-${width}.png`, fullPage: true });
      await context.close();
    }
    console.log('People: 2 viewport scenarios passed');
  } finally { await browser.close(); }
}
run().catch(error => { console.error(error); process.exitCode = 1; });

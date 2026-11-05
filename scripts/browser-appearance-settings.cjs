// Real Svelte views with synthetic APIs; no deployed Core or NAS proof.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5187/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/appearance-settings';
fs.mkdirSync(out, { recursive: true });
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const scenarios = [];
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await fixturePage(browser, width, 'dark');
      let saved = { locale: 'fr', theme: 'dark' };
      await page.route('**/api/v1/preferences', async route => {
        if (route.request().method() === 'PUT') {
          saved = { ...saved, ...route.request().postDataJSON() };
          return route.fulfill({ status: 204 });
        }
        return route.fulfill({ json: saved });
      });
      await page.route('**/api/v1/account/recommendations', route => route.fulfill({ json: {
        enabled: true, mode: 'discovery', items: [{ id: 'recommendation', kind: 'film', title: 'Découverte locale', synthetic: true, libraryId: 'film', posterUrl: '/ux-art/poster', editions: [], recommendationReason: 'À découvrir dans votre catalogue' }]
      } }));
      await page.route('**/api/v1/account/recommendations/preferences', route => route.fulfill({ json: {
        enabled: true, excludeWatched: true, historyDays: 180
      } }));
      await page.goto(base);
      const recommendation = page.getByRole('heading', { name: 'À découvrir dans votre catalogue', exact: true });
      await recommendation.waitFor();
      assert.equal(await recommendation.evaluate(heading => {
        const home = heading.closest('.catalog-home');
        const tonight = home.querySelector('.tonight');
        const shelf = heading.closest('.shelf-container');
        return Boolean(tonight && tonight.nextElementSibling === shelf);
      }), true, 'Recommendations must follow Pour ce soir');
      await page.goto(base + 'settings/#display');
      const appearance = page.getByRole('region', { name: 'Personnaliser l’affichage' });
      await appearance.getByRole('combobox', { name: 'Taille des affiches' }).waitFor();
      assert.equal(await appearance.getByRole('combobox', { name: 'Couleur d’accent' }).count(), 0);
      assert.equal(await page.getByLabel('Activer les recommandations sur l’accueil').isChecked(), true);
      await appearance.getByRole('combobox', { name: 'Taille des affiches' }).selectOption('small');
      await appearance.getByRole('combobox', { name: 'Espacement des cartes' }).selectOption('compact');
      await appearance.getByRole('button', { name: 'Monter Séries récentes', exact: true }).click();
      await Promise.all([
        page.waitForResponse(response => response.url().endsWith('/api/v1/preferences') && response.request().method() === 'PUT' && response.request().postDataJSON().homeSections?.length === 5),
        appearance.getByRole('button', { name: 'Masquer', exact: true }).first().click()
      ]);
      assert.equal(saved.cardSize, 'small');
      assert.equal(saved.cardDensity, 'compact');
      assert.deepEqual(saved.homeSections, ['continue', 'tonight', 'series', 'films', 'rated']);
      await page.reload();
      await appearance.getByRole('combobox', { name: 'Taille des affiches' }).waitFor();
      assert.equal(await appearance.getByRole('combobox', { name: 'Taille des affiches' }).inputValue(), 'small');
      assert.equal(await appearance.getByRole('button', { name: 'Afficher : À la une', exact: true }).count(), 1);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await page.screenshot({ path: `${out}/settings-${width}.png`, fullPage: true });
      await page.goto(base);
      await page.locator('.catalog-home').waitFor();
      assert.equal(await page.locator('.hero-banner').count(), 0);
      await page.screenshot({ path: `${out}/home-${width}.png`, fullPage: true });
      assert.deepEqual(errors, []);
      scenarios.push({ width, persistence: true, order: saved.homeSections, overflow: false });
      await context.close();
    }
  } finally { await browser.close(); }
  fs.writeFileSync(`${out}/report.json`, JSON.stringify({ passed: true, scenarios }, null, 2));
  console.log(JSON.stringify(scenarios));
}
run().catch(error => { console.error(error); process.exitCode = 1; });

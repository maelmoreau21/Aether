// Real Svelte UI with synthetic APIs; no Core/Node execution or NAS proof.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/ux-tasks';
fs.mkdirSync(out, { recursive: true });

async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { scope: 'Synthetic APIs, real frontend', browser: browser.version(), scenarios: [] };
  try {
    for (const theme of ['dark', 'light']) for (const width of [390, 1440]) {
      const { page, context, errors, requests } = await setup(browser, width, theme);
      let trims = 0;
      const memory = {
        core: { rssBytes: 10485760, peakRssBytes: 20971520, anonBytes: 5242880, fileBytes: 5242880 },
        childrenTotalRssBytes: 0, children: [], host: { linuxReclaimableCacheBytes: 52428800 },
        summary: { diagnosis: 'Diagnostic synthétique du cache de pages Linux', reclaimableAdvice: 'Conseil synthétique à masquer' }
      };
      await page.route('**/api/v1/admin/system/memory**', route => {
        if (route.request().method() === 'POST') {
          trims++;
          assert.equal(route.request().headers()['x-csrf-token'], 'synthetic-test');
        }
        return route.fulfill({ json: memory });
      });
      await page.goto(base + 'admin/libraries/#tasks-section');
      await page.getByRole('heading', { name: 'Tâches et indexations', exact: true }).waitFor();
      const scan = page.getByRole('link', { name: 'Configurer Indexer Films de voyage — Bibliothèque personnelle', exact: true });
      await scan.waitFor();
      assert.equal(await page.getByText('File d’exécution et opérations en cours', { exact: true }).count(), 0);
      assert.equal(await page.getByText('Automatisation des métadonnées', { exact: true }).count(), 0);
      assert.equal(await page.locator('.queue-section,.automation-section').count(), 0);
      assert.equal(requests.some(url => /\/tasks\/(jobs|automation)\b/.test(url)), false, 'Removed panels must not fetch their APIs');
      const summary = page.locator('.plans-section > summary');
      await summary.focus();
      await page.keyboard.press('Enter');
      assert.equal(await page.locator('.plans-section').getAttribute('open'), null);
      await page.keyboard.press('Enter');
      await scan.click();
      await page.getByRole('heading', { name: 'Indexer Films de voyage — Bibliothèque personnelle', exact: true }).waitFor();
      await page.getByRole('button', { name: 'Ajouter un intervalle', exact: true }).click();
      await page.getByRole('dialog', { name: 'Déclencheur périodique', exact: true }).waitFor();
      await page.keyboard.press('Escape');
      await page.goto(base + 'admin/libraries/#tasks-section');
      await scan.waitFor();
      await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo(0, 0); });
      await page.screenshot({ path: `${out}/${theme}-${width}-plans.png`, fullPage: true });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);

      await page.goto(base + 'admin/libraries/#backups-section');
      const panel = page.locator('#backups-section');
      const trim = panel.getByRole('button', { name: 'Libérer la mémoire (Trim)', exact: true });
      await trim.waitFor();
      assert.equal(await panel.getByText('Core Aether (RSS)', { exact: true }).count(), 1);
      assert.equal(await panel.getByText('Cache disque Linux (OS)', { exact: true }).count(), 1);
      assert.equal(await panel.getByText('Diagnostic :', { exact: true }).count(), 0);
      assert.equal(await panel.getByText(memory.summary.diagnosis, { exact: true }).count(), 0);
      assert.equal(await panel.getByText(memory.summary.reclaimableAdvice, { exact: true }).count(), 0);
      await trim.click();
      await panel.getByText('Caches purgés et mémoire restituée au système d’exploitation.', { exact: true }).waitFor();
      assert.equal(trims, 1);
      await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo(0, 0); });
      await page.screenshot({ path: `${out}/${theme}-${width}-memory.png`, fullPage: true });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      assert.deepEqual(errors, []);
      report.scenarios.push({ theme, width, removedPanels: true, noQueuePolling: true, schedulingAccessible: true, keyboard: true, memoryMessageHidden: true, metricsKept: true, trimWorks: true, overflow: false });
      await context.close();
    }
    report.passed = true;
  } catch (error) {
    report.failure = String(error.stack || error);
    throw error;
  } finally {
    fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
    await browser.close();
    console.log(JSON.stringify(report));
  }
}
if (require.main === module) run().catch(error => { console.error(error); process.exitCode = 1; });

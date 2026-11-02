// Real Svelte UI with synthetic APIs: this does not validate Core persistence or media decoding.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/playback-settings';
fs.mkdirSync(out, { recursive: true });

async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { scope: 'Svelte settings; synthetic APIs, no Core/NAS/media proof', scenarios: [] };
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await fixturePage(browser, width, 'dark');
      let saved = { locale: 'fr', theme: 'dark' };
      const writes = [];
      await page.route('**/api/v1/preferences', async route => {
        if (route.request().method() === 'PUT') {
          const patch = route.request().postDataJSON();
          writes.push(patch);
          saved = { ...saved, ...patch };
          return route.fulfill({ status: 204 });
        }
        return route.fulfill({ json: saved });
      });
      await page.goto(base + 'settings/#playback');
      const autoplay = page.getByRole('checkbox', { name: 'Enchaîner automatiquement l’épisode suivant', exact: true });
      const back = page.getByRole('combobox', { name: /^Reculer de/ });
      const forward = page.getByRole('combobox', { name: /^Avancer de/ });
      await autoplay.waitFor();
      assert.equal(await autoplay.isChecked(), false);
      assert.equal(await back.inputValue(), '10');
      assert.equal(await forward.inputValue(), '10');
      await autoplay.check();
      await back.selectOption('5');
      await Promise.all([
        page.waitForResponse(response => response.url().endsWith('/api/v1/preferences') && response.request().method() === 'PUT' && response.request().postDataJSON().skipForwardSeconds === 30),
        forward.selectOption('30')
      ]);
      assert.equal(saved.autoplayNextEpisode, true);
      assert.equal(saved.skipBackSeconds, 5);
      assert.equal(saved.skipForwardSeconds, 30);
      await page.reload();
      await autoplay.waitFor();
      assert.equal(await autoplay.isChecked(), true);
      assert.equal(await back.inputValue(), '5');
      assert.equal(await forward.inputValue(), '30');
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await page.screenshot({ path: `${out}/settings-${width}.png`, fullPage: true });
      assert.deepEqual(errors, []);
      report.scenarios.push({ width, defaults: true, writes: writes.length, reload: true, overflow: false });
      await context.close();
    }
    report.passed = true;
  } finally {
    fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
    await browser.close();
  }
  console.log(JSON.stringify(report));
}
if (require.main === module) run().catch(error => { console.error(error); process.exitCode = 1; });

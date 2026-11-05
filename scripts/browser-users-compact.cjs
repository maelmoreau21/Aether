// Admin users/groups, served by Vite with synthetic APIs.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5193/app/';
const out = 'output/playwright/users-compact';
fs.mkdirSync(out, { recursive: true });
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await setup(browser, width, 'dark');
      await page.goto(base + 'admin/libraries/#users-section');
      const users = page.locator('#users-section');
      await users.getByRole('searchbox', { name: 'Rechercher un utilisateur' }).waitFor();
      assert.equal(await users.getByText('Gérez les comptes d’accès locaux', { exact: false }).count(), 0);
      const role = users.getByLabel('Filtrer par rôle');
      const status = users.getByLabel('Filtrer par statut');
      const [r, s] = await Promise.all([role.boundingBox(), status.boundingBox()]);
      assert.ok(Math.abs(r.y - s.y) < 1, 'Filters must be on the same row');
      assert.ok(r.height <= 46 && s.height <= 46);
      await users.getByRole('searchbox').fill('lecteur');
      await role.selectOption('user');
      await status.selectOption('active');
      assert.equal(await users.locator('.main-table tbody tr').count(), 1);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await page.screenshot({ path: `${out}/users-${width}.png`, fullPage: true });
      await page.goto(base + 'admin/libraries/#groups-section');
      const groups = page.locator('#groups-section');
      await groups.getByRole('button', { name: 'Créer', exact: true }).waitFor();
      assert.equal(await groups.getByText('Organisez vos utilisateurs', { exact: false }).count(), 0);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      assert.deepEqual(errors, []);
      await context.close();
      console.log(`PASS ${width}`);
    }
  } finally { await browser.close(); }
}
run().catch(error => { console.error(error); process.exitCode = 1; });

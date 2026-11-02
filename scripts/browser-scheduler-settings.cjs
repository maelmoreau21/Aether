// Real Svelte interactions; synthetic APIs, no Core/Node execution proof.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/scheduler-settings';

async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { scope: 'Synthetic API contracts; browser UI only', scenarios: [] };
  fs.mkdirSync(out, { recursive: true });
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await setup(browser, width, 'dark');
      const writes = [];
      let plans = [], queueReads = 0;
      const task = { id: 'clean-cache', name: 'Nettoyer le cache', description: 'Nettoyage synthétique', category: 'Maintenance', enabled: 1, intervalMinutes: 1440, nextDueAt: 1793660400, lastStartedAt: 1793570400, lastCompletedAt: 1793570410, lastOutcome: 'failed', lastErrorKind: 'disk', lastErrorScope: 'image-cache', lastMessage: 'Échec de nettoyage synthétique' };
      const backup = { available: true, running: false, archives: [], schedule: { enabled: false, intervalMinutes: 1440, nextDueAt: 1793660400, lastStartedAt: null, lastCompletedAt: null, lastOutcome: 'never' } };
      await page.route('**/api/v1/admin/tasks/**', async route => {
        const request = route.request(), path = new URL(request.url()).pathname;
        if (request.method() === 'GET') {
          if (path.endsWith('/schedules')) return route.fulfill({ json: plans });
          if (path.endsWith('/system')) return route.fulfill({ json: [task] });
          if (path.endsWith('/jobs')) { queueReads++; return route.fulfill({ json: { jobs: [], limit: 50, offset: 0 } }); }
          return route.fallback();
        }
        const body = request.postData() ? request.postDataJSON() : null;
        writes.push({ path, method: request.method(), body });
        if (path.endsWith('/schedules/lib-film')) plans = [{ libraryId: 'lib-film', ...body, enabled: Number(body.enabled), nextRunAt: 1793660400, lastRunAt: null, lastOutcome: 'never' }];
        if (path.endsWith('/clean-cache/schedule')) Object.assign(task, body, { enabled: Number(body.enabled) });
        if (path.endsWith('/clean-cache/run')) task.lastOutcome = 'queued';
        return route.fulfill({ status: path.endsWith('/run') ? 202 : 204 });
      });
      await page.route('**/api/v1/admin/backups**', async route => {
        if (route.request().method() === 'GET') return route.fulfill({ json: backup });
        const body = route.request().postDataJSON();
        writes.push({ path: new URL(route.request().url()).pathname, method: route.request().method(), body });
        Object.assign(backup.schedule, body);
        return route.fulfill({ status: 204 });
      });
      await page.goto(base + 'admin/libraries/#tasks-section');
      const scanName = 'Indexer Films de voyage — Bibliothèque personnelle';
      await page.getByRole('link', { name: `Configurer ${scanName}`, exact: true }).waitFor();
      assert.equal(await page.locator('.plans-section').getAttribute('open'), '');
      assert.equal(queueReads, 0, 'closed execution queue must not poll');
      await page.getByRole('link', { name: `Configurer ${scanName}`, exact: true }).click();
      await page.getByRole('button', { name: 'Ajouter un intervalle', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Déclencheur périodique' });
      await dialog.getByRole('combobox', { name: 'Unité', exact: true }).selectOption('1');
      await dialog.getByRole('spinbutton', { name: 'Toutes les', exact: true }).fill('15');
      await dialog.getByRole('button', { name: 'Enregistrer', exact: true }).click();
      await page.getByRole('button', { name: 'Désactiver l’intervalle', exact: true }).click();
      await page.getByRole('button', { name: 'Configurer l’intervalle', exact: true }).click();
      assert.equal(await dialog.getByRole('checkbox', { name: 'Activer cet intervalle', exact: true }).isChecked(), false);
      await dialog.getByRole('checkbox', { name: 'Activer cet intervalle', exact: true }).check();
      await dialog.getByRole('button', { name: 'Enregistrer', exact: true }).click();
      assert(writes.some(w => w.method === 'PUT' && w.body.intervalMinutes === 15 && w.body.enabled === false));
      assert(!writes.some(w => w.method === 'DELETE'));
      await page.goto(base + 'admin/libraries/#tasks-section?task=system-clean-cache');
      await page.getByText('Échec de nettoyage synthétique', { exact: true }).waitFor();
      await page.getByRole('button', { name: 'Exécuter Nettoyer le cache', exact: true }).click();
      await page.locator('.results').getByText('En attente', { exact: true }).waitFor();
      task.lastOutcome = 'succeeded'; task.lastMessage = 'Nettoyage synthétique terminé';
      if (width === 390) await page.locator('.results').getByText('Réussie', { exact: true }).waitFor({ timeout: 15000 });
      else { await page.getByRole('button', { name: 'Actualiser l’état', exact: true }).click(); await page.locator('.results').getByText('Réussie', { exact: true }).waitFor(); }
      await page.goto(base + 'admin/libraries/#tasks-section?task=backup');
      await page.getByRole('button', { name: 'Configurer l’intervalle', exact: true }).click();
      await dialog.getByRole('combobox', { name: 'Unité', exact: true }).selectOption('1');
      await dialog.getByRole('spinbutton', { name: 'Toutes les', exact: true }).fill('30');
      assert.equal(await dialog.getByRole('spinbutton', { name: 'Toutes les', exact: true }).evaluate(input => input.checkValidity()), false);
      await dialog.getByRole('spinbutton', { name: 'Toutes les', exact: true }).fill('60');
      await dialog.getByRole('checkbox', { name: 'Activer cet intervalle', exact: true }).check();
      await dialog.getByRole('button', { name: 'Enregistrer', exact: true }).click();
      assert(writes.some(w => w.path.endsWith('/backups/schedule') && w.body.intervalMinutes === 60 && w.body.enabled));
      await dialog.waitFor({ state: 'hidden' });
      await page.screenshot({ path: `${out}/scheduler-${width}.png`, fullPage: true });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      assert.deepEqual(errors, []);
      report.scenarios.push({ width, scanInterval: true, preservedDisabledPlan: true, systemQueueAndLiveStatus: true, backupBounds: true });
      await context.close();
    }
  } finally { fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2)); await browser.close(); }
  console.log('Scheduler scenarios passed:', report.scenarios.length);
}
run().catch(error => { console.error(error); process.exitCode = 1; });

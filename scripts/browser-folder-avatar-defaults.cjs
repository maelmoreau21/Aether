// Real Svelte UI with explicit synthetic APIs; does not prove NAS filesystem access.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5197/app/';
const out = 'output/playwright/folder-avatar-defaults';
fs.mkdirSync(out, { recursive: true });

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    for (const width of [390, 1440]) {
      const { page, context, errors } = await setup(browser, width, 'dark');
      const browses = [];
      await page.route('**/api/v1/admin/roots', route => route.fulfill({ json: {
        roots: ['/tmp/aether-core-empty-local-root'], localRoots: ['/tmp/aether-core-empty-local-root']
      } }));
      await page.route('**/api/v1/admin/filesystem/browse?**', route => {
        const url = new URL(route.request().url());
        browses.push({ nodeId: url.searchParams.get('nodeId'), path: url.searchParams.get('path') });
        return route.fulfill({ json: { path: url.searchParams.get('path') || '/', parent: null, directories: [] } });
      });
      await page.goto(base + 'admin/libraries/#libraries-section');
      await page.getByRole('button', { name: 'Ajouter une bibliothèque', exact: true }).click();
      await page.getByRole('button', { name: 'Configurer les dossiers', exact: true }).click();
      assert.equal(await page.locator('#library-root').inputValue(), '');
      await page.getByRole('button', { name: 'Parcourir…', exact: true }).click();
      const picker = page.getByRole('dialog', { name: 'Sélectionner un chemin' });
      await picker.getByRole('button', { name: 'Sélectionner ce dossier' }).waitFor();
      assert.equal(await page.locator('#picker-path-input').inputValue(), '/');
      assert.deepEqual(browses.at(-1), { nodeId: 'local', path: '' });
      await page.keyboard.press('Escape');
      await page.locator('#library-root').fill('/media/films');
      await page.getByRole('button', { name: 'Parcourir…', exact: true }).click();
      await page.waitForFunction(() => document.querySelector('#picker-path-input')?.value === '/media/films');
      assert.equal(browses.at(-1).path, '/media/films');
      await page.keyboard.press('Escape');
      await page.keyboard.press('Escape');
      await page.getByRole('button', { name: 'Gérer Films de voyage — Bibliothèque personnelle', exact: true }).click();
      await page.getByRole('button', { name: 'Consulter les dossiers', exact: true }).click();
      assert.equal(await page.locator('#library-root').inputValue(), '');
      await page.getByRole('button', { name: 'Parcourir…', exact: true }).click();
      await page.waitForFunction(() => document.querySelector('#picker-path-input')?.value === '/');
      assert.deepEqual(browses.at(-1), { nodeId: 'local', path: '' });
      await page.screenshot({ path: `${out}/root-${width}.png` });
      await page.keyboard.press('Escape');
      await page.keyboard.press('Escape');
      // Required local fallback keeps assignment available even with an empty gallery.
      await page.goto(base + 'admin/libraries/#avatars-section');
      const assign = page.getByRole('button', { name: 'Attribuer les avatars manquants maintenant' });
      await assign.waitFor();
      assert.equal(await page.getByLabel('Autoriser les images personnalisées').count(), 0);
      assert.equal(await page.getByLabel('Attribuer un avatar de la galerie aux nouveaux comptes').count(), 0);
      assert.equal(await assign.isDisabled(), false);
      assert.equal(await assign.evaluate(button => getComputedStyle(button).cursor), 'pointer');
      await page.getByText('La galerie de secours locale reste disponible pour tous les profils.').waitFor();
      await page.route('**/api/v1/admin/avatar-packs', route => route.fulfill({ json: {
        packs: [{ id: 'native-orbits', name: 'Orbites', count: 12 }]
      } }));
      await page.route('**/api/v1/admin/avatars/assign-missing', async route => {
        await new Promise(resolve => setTimeout(resolve, 500));
        return route.fulfill({ json: { count: 2 } });
      });
      await page.reload();
      await assign.waitFor();
      await page.waitForFunction(() => ![...document.querySelectorAll('button')].find(button => button.textContent === 'Attribuer les avatars manquants maintenant')?.disabled);
      await assign.click();
      assert.equal(await assign.evaluate(button => getComputedStyle(button).cursor), 'wait');
      await page.getByText('2 comptes ont reçu un avatar.').waitFor();
      assert.equal(await assign.evaluate(button => getComputedStyle(button).cursor), 'pointer');
      assert.deepEqual(errors, []);
      await context.close();
      console.log('PASS folder defaults, explicit paths and avatar cursors', width);
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });


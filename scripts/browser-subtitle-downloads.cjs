// Production Svelte UI, synthetic APIs only: no provider account, Core, or playback proof.
const assert = require('node:assert/strict');
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-details.cjs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const { page, context, errors } = await setup(browser, 1440, 'dark');
    let downloaded = false, searches = 0, mutations = 0;
    await page.route('**/api/v1/admin/subtitles/**', route => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (request.method() !== 'GET') {
        assert.equal(request.headers()['x-csrf-token'], 'synthetic-test');
        mutations++;
      }
      if (path.endsWith('/search')) {
        const body = request.postDataJSON();
        assert.equal(body.providerId, 'opensubtitles');
        assert.equal(body.language, 'fr');
        searches++;
        return route.fulfill({ json: { searchId: 'search-' + body.page, items: [{ id: 'file-' + body.page, language: 'fr', label: 'Sous-titre de test ' + body.page, forced: false, sdh: true, match: 'identity' }], totalPages: 2 } });
      }
      if (path.endsWith('/download')) {
        assert.deepEqual(request.postDataJSON(), { searchId: 'search-2', fileId: 'file-2' });
        downloaded = true;
        return route.fulfill({ json: { id: 'download-1', status: 'completed' } });
      }
      if (request.method() === 'DELETE') {
        downloaded = false;
        return route.fulfill({ status: 204 });
      }
      return route.fulfill({ json: { enabled: true, providers: [{ id: 'opensubtitles', name: 'OpenSubtitles' }], quotas: [{ providerId: 'opensubtitles', remaining: downloaded ? 4 : 5, resetTimeUtc: null, nextAllowedAt: 0 }], downloads: downloaded ? [{ id: 'download-1', providerId: 'opensubtitles', fileId: 'file-2', language: 'fr', label: 'Sous-titre de test 2', forced: false, sdh: true, status: 'completed' }] : [] } });
    });
    await page.goto(base + 'films/?work=film-0');
    await page.getByRole('button', { name: "Plus d'actions et informations" }).click();
    await page.getByRole('menuitem', { name: 'Rechercher des sous-titres' }).click();
    const dialog = page.getByRole('dialog', { name: /Sous-titres ·/ });
    await dialog.getByText(/Quota du fournisseur : 5 téléchargement/).waitFor();
    await dialog.getByRole('button', { name: 'Rechercher', exact: true }).click();
    await dialog.getByText('Correspondance par identifiant ; synchronisation non garantie.', { exact: true }).waitFor();
    assert.equal(searches, 1);
    assert.equal(downloaded, false, 'search does not download automatically');
    await dialog.getByRole('button', { name: 'Suivants', exact: true }).click();
    await dialog.getByText('Sous-titre de test 2', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: 'Télécharger', exact: true }).click();
    await dialog.getByText(/Sous-titre téléchargé\. Relancez le lecteur/).waitFor();
    assert.equal(downloaded, true);
    await dialog.getByText(/Quota du fournisseur : 4 téléchargement/).waitFor();
    assert.equal(await dialog.getByRole('button', { name: 'Télécharger', exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: 'Supprimer le téléchargement', exact: true }).click();
    await dialog.getByText('Aucun sous-titre téléchargé pour cette copie.', { exact: true }).waitFor();
    assert.equal(downloaded, false);
    assert.equal(mutations, 4);
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'hidden' });
    await page.route('**/api/v1/auth/me', route => route.fulfill({ json: { user: { id: 'user', login: 'user', role: 'user' }, csrfToken: 'synthetic-user' } }));
    await page.reload();
    await page.getByRole('button', { name: "Plus d'actions et informations" }).click();
    assert.equal(await page.getByRole('menuitem', { name: 'Rechercher des sous-titres' }).count(), 0);
    assert.deepEqual(errors, []);
    console.log(`PASS Chrome ${browser.version()}: subtitle UI explicit search, pagination, CSRF, download, duplicate button, delete, Escape, user action hidden. Synthetic APIs only; no Core/provider/playback proof.`);
    await context.close();
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });

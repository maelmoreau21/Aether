// UI regression with synthetic APIs and a real img-src 'self' policy.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-details.cjs');
const assert = require('node:assert/strict');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j5S8AAAAASUVORK5CYII=', 'base64');
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const { page, context, errors } = await setup(browser, 1440, 'dark');
    let previews = 0, remoteRequests = 0;
    await page.route('**/app/films/**', async route => {
      const response = await route.fetch();
      return route.fulfill({ response, headers: { ...response.headers(), 'content-security-policy': "img-src 'self' data:" } });
    });
    await page.route('https://image.tmdb.org/**', route => { remoteRequests++; return route.abort(); });
    await page.route('**/api/v1/admin/extensions/*/image-preview?**', route => {
      const url = new URL(route.request().url());
      assert.equal(url.searchParams.get('workId'), 'film-0');
      assert.equal(url.searchParams.get('url'), 'https://image.tmdb.org/t/p/w185/test.png');
      previews++;
      return route.fulfill({ contentType: 'image/png', body: png });
    });
    await page.route('**/api/v1/admin/extensions/identify/search', route => route.fulfill({ json: {
      results: [{ provider: 'tmdb', id: '1', title: 'Résultat', posterUrl: 'https://image.tmdb.org/t/p/w185/test.png' }], providers: [{ id: 'tmdb', name: 'TMDb' }], errors: []
    } }));
    await page.route('**/api/v1/admin/extensions/images/candidates', route => route.fulfill({ json: {
      images: [{ provider: 'tmdb', imageType: 'poster', url: 'https://image.tmdb.org/t/p/w185/test.png' }], providers: [{ id: 'tmdb', name: 'TMDb' }], errors: []
    } }));
    await page.goto(base + 'films/?work=film-0');
    await page.getByRole('button', { name: "Plus d'actions et informations" }).click();
    await page.getByRole('menuitem', { name: 'Identifier', exact: true }).click();
    const identify = page.getByRole('dialog', { name: /Identifier/ });
    await identify.getByRole('button', { name: 'Rechercher', exact: true }).click();
    await identify.getByRole('img', { name: 'Affiche de Résultat' }).evaluate(img => img.decode());
    await identify.getByRole('button', { name: 'Fermer', exact: true }).click();
    await page.getByRole('button', { name: "Plus d'actions et informations" }).click();
    await page.getByRole('menuitem', { name: 'Modifier les images', exact: true }).click();
    const images = page.getByRole('dialog', { name: /Modifier les images/ });
    await images.getByRole('button', { name: 'Rechercher des affiches', exact: true }).click();
    await page.getByRole('img', { name: 'Aperçu poster', exact: true }).evaluate(img => img.decode());
    // Chrome may reuse the decoded image between both dialogs.
    assert.ok(previews >= 1 && previews <= 2);
    assert.equal(remoteRequests, 0);
    assert.deepEqual(errors, []);
    console.log('PASS identification and image picker previews with img-src self; no browser request to provider. Synthetic API responses only.');
    await context.close();
  } finally { await browser.close(); }
}
run().catch(error => { console.error(error); process.exitCode = 1; });

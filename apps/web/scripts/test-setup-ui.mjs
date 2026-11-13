import { chromium } from 'playwright';
import assert from 'node:assert/strict';

const origin = process.env.MEDIA_TEST_ORIGIN ?? 'http://127.0.0.1:5187';
const base = process.env.MEDIA_WEB_TEST_URL ?? `${origin}/app`;

const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, locale: 'fr-FR' });
  const errors = [];
  page.on('pageerror', err => errors.push(String(err)));

  let bootstrapped = false;
  let checkedPath = null;
  let createdLibrary = null;
  let scannedId = null;

  await page.route('**/api/v1/**', async route => {
    const req = route.request();
    const url = new URL(req.url());
    const path = url.pathname;
    const method = req.method();
    const json = (body, status = 200) =>
      route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });

    if (path.endsWith('/auth/me')) {
      return bootstrapped
        ? json({ user: { id: 'admin-1', login: 'admin', role: 'administrator', displayName: 'Admin' }, csrfToken: 'csrf-setup' })
        : json({ error: 'unauthorized' }, 401);
    }
    if (path.endsWith('/auth/bootstrap/status')) {
      return json({ needsBootstrap: !bootstrapped, hasLibraries: Boolean(createdLibrary), localLoginEnabled: true, httpBootstrapEnabled: true });
    }
    if (path.endsWith('/auth/bootstrap') && method === 'POST') {
      bootstrapped = true;
      return json({ user: { id: 'admin-1', login: 'admin', role: 'administrator', displayName: 'Admin' }, csrfToken: 'csrf-setup' });
    }
    if (path.endsWith('/admin/roots')) {
      return json({ roots: ['/media/films'], localRoots: ['/media/films'] });
    }
    if (path.endsWith('/admin/filesystem/check') && method === 'POST') {
      const payload = req.postDataJSON();
      checkedPath = payload.path;
      return json({ readable: true, path: payload.path, entriesCount: 12, mediaFilesCount: 8 });
    }
    if (path.endsWith('/admin/libraries') && method === 'POST') {
      createdLibrary = req.postDataJSON();
      return json({ id: 'lib-1', ...createdLibrary }, 201);
    }
    if (path.endsWith('/admin/libraries/lib-1/scan') && method === 'POST') {
      scannedId = 'lib-1';
      return json({ id: 'scan-1' }, 202);
    }
    if (path.endsWith('/config')) return json({ name: 'Aether' });
    return json({});
  });

  await page.goto(`${base}/setup/`);
  await page.getByRole('heading', { name: 'Créer l\'administrateur' }).waitFor();
  await page.locator('#setup-token').fill('bootstrap-token-demo');
  await page.locator('#setup-login').fill('admin');
  await page.locator('#setup-password').fill('very-secure-admin-password-15');
  await page.locator('#setup-confirm').fill('very-secure-admin-password-15');
  await page.getByRole('button', { name: 'Initialiser le serveur' }).click();

  await page.getByRole('heading', { name: 'Ajouter votre première médiathèque' }).waitFor();
  assert.equal(await page.locator('#setup-lib-path').inputValue(), '/media/films');
  await page.getByRole('button', { name: 'Tester les permissions de lecture' }).click();
  await page.getByText('Dossier lisible (12 entrées, 8 fichiers média détectés).').waitFor();
  assert.equal(checkedPath, '/media/films');

  await page.getByRole('button', { name: 'Créer et lancer l’analyse' }).click();
  await page.getByRole('heading', { name: 'Votre serveur Aether est prêt' }).waitFor();
  assert.equal(createdLibrary?.name, 'Films');
  assert.equal(createdLibrary?.kind, 'films');
  assert.equal(createdLibrary?.root, '/media/films');
  assert.equal(scannedId, 'lib-1');
  assert.deepEqual(errors, []);
  console.log('PASS: setup wizard (admin bootstrap + folder permission test + first library + initial scan).');
} finally {
  await browser.close();
}

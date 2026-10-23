import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';

const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !process.env.MEDIA_TEST_PACKAGE || !process.env.MEDIA_TEST_PUBLIC_KEY) throw new Error('Use the Rust extensions browser harness.');
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, colorScheme: 'dark' });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('response', async response => {
    if (response.url().includes('/api/v1/admin/extensions') && !response.ok()) console.log('Extension API failure:', response.status(), await response.text());
  });
  const response = await page.request.post(`${origin}/api/v1/auth/login`, { headers: { origin }, data: { login: 'extension-admin', password: 'synthetic extension password' } });
  assert.equal(response.status(), 200);
  const { csrfToken } = await response.json();
  await page.goto(`${origin}/app/admin/libraries/#extensions-section`);
  const panel = page.locator('#extensions-section');
  await panel.getByRole('button', {name:'Installées',exact:true}).click();
  await panel.getByText('Aucune extension installée.', { exact: true }).waitFor();
  const imported = await page.request.post(`${origin}/api/v1/admin/extensions`, {
    headers: {origin, 'x-csrf-token':csrfToken},
    data: {...JSON.parse(await readFile(process.env.MEDIA_TEST_PACKAGE,'utf8')),publicKey:process.env.MEDIA_TEST_PUBLIC_KEY}
  });
  assert.equal(imported.status(),201);
  // Synthetic catalogue exercises discovery without downloading a remote repository.
  const signedPackage = JSON.parse(await readFile(process.env.MEDIA_TEST_PACKAGE, 'utf8'));
  const catalogueRoute = '**/api/v1/admin/extensions/catalogue';
  await page.route(catalogueRoute, route => route.fulfill({ json: { entries: [
    { repositoryId: 'test', repositoryName: 'Dépôt synthétique', compatible: true,
      manifest: signedPackage.manifest, publisherId: 'test', publicKey: process.env.MEDIA_TEST_PUBLIC_KEY },
    ...['Avatars', 'Illustrations', 'Musique'].map((name, index) => ({ repositoryId: 'test', repositoryName: 'Dépôt synthétique', compatible: index !== 2, manifest: {...signedPackage.manifest, id: `preview-${index}`, name: `${name} · Démo`, description: 'Aperçu synthétique du catalogue, sans extension installable.', category: name, permissions: index === 0 ? ['avatars'] : ['metadata.read']}, publisherId: 'test', publicKey: process.env.MEDIA_TEST_PUBLIC_KEY }))
  ] } }));
  await page.reload();
  await panel.getByRole('button', {name:'Catalogue',exact:true}).click();
  await panel.locator('.artwork-button').first().waitFor();
  await panel.getByRole('button', {name:'Avatars',exact:true}).click();
  assert.equal(await panel.locator('.catalogue-card').count(), 1);
  await panel.getByRole('button', {name:'Tout',exact:true}).click();
  const outputPreview = process.env.MEDIA_REPORT_DIR || '../../target/extensions-lifecycle-browser';
  await mkdir(outputPreview, { recursive: true });
  await page.screenshot({ path: `${outputPreview}/catalogue-desktop.png`, fullPage: true });
  await panel.getByRole('button', {name: /^Détails de/}).first().click();
  await panel.getByRole('heading', {name: 'Versions disponibles'}).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.screenshot({ path: `${outputPreview}/catalogue-detail-mobile.png`, fullPage: true });
  await panel.getByRole('button', {name: 'Retour au catalogue'}).click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.screenshot({ path: `${outputPreview}/catalogue-mobile.png`, fullPage: true });
  await panel.getByRole('searchbox').fill('introuvable-synthetique');
  await panel.getByText('Aucune extension ne correspond.', {exact:false}).waitFor();
  await panel.getByRole('searchbox').fill('');
  await panel.getByRole('button', {name:'Dépôts',exact:true}).click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.screenshot({ path: `${outputPreview}/repositories-mobile.png`, fullPage: true });
  await panel.getByRole('button', {name:'+ Nouveau dépôt',exact:true}).click();
  await panel.getByLabel('Adresse du dépôt', {exact:true}).waitFor();
  await panel.getByRole('button', {name:'Fermer',exact:true}).click();
  await page.unroute(catalogueRoute);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await panel.getByRole('button', {name:'Installées',exact:true}).click();
  const extension = panel.locator('.extension-detail');
  await panel.locator('.installed-card').getByRole('button', {name:/^Paramètres de/}).click();
  await extension.getByRole('switch').check();
  await panel.getByText('Permissions enregistrées et extension activée.', { exact: true }).waitFor();
  await extension.getByText('Tester ce fournisseur sur une œuvre',{exact:true}).click();
  await extension.getByRole('button', { name: 'Rechercher dans le catalogue', exact: true }).click();
  await extension.getByLabel('Œuvre à analyser', { exact: true }).selectOption('work-films');
  await extension.getByRole('button', { name: 'Exécuter sur cette œuvre', exact: true }).click();
  await panel.getByText('Proposition générée. Consultez les résultats ci-dessous.', { exact: true }).waitFor();
  await panel.locator('strong').filter({ hasText: /^Titre original$/ }).first().waitFor();
  await panel.getByText(/Proposition synthétique pour work-films/).first().waitFor();
  await page.reload();
  await panel.getByRole('button', {name:'Installées',exact:true}).click();
  await panel.getByText(/^Dernières exécutions/).click();
  await panel.locator('strong').filter({ hasText: /^Titre original$/ }).waitFor();
  await panel.locator('.installed-card').getByRole('button', {name:/^Paramètres de/}).click();
  const output = process.env.MEDIA_REPORT_DIR || '../../target/extensions-lifecycle-browser';
  await mkdir(output, { recursive: true });
  await page.screenshot({ path: `${output}/desktop.png`, fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: `${output}/mobile.png`, fullPage: true });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await extension.getByRole('switch').uncheck();
  await panel.getByText('Extension désactivée.', { exact: true }).waitFor();
  const denied = await page.request.post(`${origin}/api/v1/admin/extensions/test.metadata/run`, { headers: { origin, 'x-csrf-token': csrfToken }, data: { workId: 'work-films' } });
  assert.equal(denied.status(), 403);
  await extension.getByText('Désinstaller', { exact: true }).click();
  await extension.getByLabel('Supprimer aussi les résultats conservés', { exact: true }).check();
  await extension.getByRole('button', { name: 'Désinstaller cette extension', exact: true }).click();
  await panel.getByText('Aucune extension installée.', { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log('Extension signed import, grants, real Rust Wasm execution, persisted proposal, disable and uninstall passed (desktop/mobile).');
} finally { await browser.close(); }

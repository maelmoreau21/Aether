import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';

const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !process.env.MEDIA_TEST_PACKAGE || !process.env.MEDIA_TEST_PUBLIC_KEY) throw new Error('Use the Rust extensions browser harness.');
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
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
  await page.reload();
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

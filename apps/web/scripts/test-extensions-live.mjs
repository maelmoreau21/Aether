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
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({ path: `${outputPreview}/catalogue-desktop.png`, fullPage: true });
  await panel.getByRole('button', {name: /^Détails de/}).first().click();
  await panel.getByRole('heading', {name: 'Versions disponibles'}).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({ path: `${outputPreview}/catalogue-detail-mobile.png`, fullPage: true });
  await panel.getByRole('button', {name: 'Retour au catalogue'}).click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({ path: `${outputPreview}/catalogue-mobile.png`, fullPage: true });
  await panel.getByRole('searchbox').fill('introuvable-synthetique');
  await panel.getByText('Aucune extension ne correspond.', {exact:false}).waitFor();
  await panel.getByRole('searchbox').fill('');
  await panel.getByRole('button', {name:'Dépôts',exact:true}).click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({ path: `${outputPreview}/repositories-mobile.png`, fullPage: true });
  await panel.getByRole('button', {name:'+ Nouveau dépôt',exact:true}).click();
  await panel.getByLabel('Adresse du dépôt', {exact:true}).waitFor();
  await panel.getByRole('button', {name:'Fermer',exact:true}).click();
  await page.unroute(catalogueRoute);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await panel.getByRole('button', {name:'Installées',exact:true}).click();
  const extension = panel.locator('.extension-detail');
  await panel.locator('.installed-card .artwork-button').click();
  await extension.getByRole('switch').check();
  await panel.getByText('Permissions enregistrées et extension activée.', { exact: true }).waitFor();
  await extension.getByRole('button',{name:'Paramètres',exact:true}).click();
  await extension.getByRole('heading',{name:'Paramètres de Test metadata'}).waitFor();
  assert.equal(await extension.getByText('Tester ce fournisseur sur une œuvre',{exact:true}).count(),0);
  assert.equal(await panel.getByText('Journal des opérations',{exact:true}).count(),0);
  await extension.getByRole('button',{name:'Retour à la fiche'}).click();
  // Execution still works through the existing API after removal of developer panels.
  const run=await page.request.post(`${origin}/api/v1/admin/extensions/test.metadata/run`,{headers:{origin,'x-csrf-token':csrfToken},data:{workId:'work-films'}});
  assert.equal(run.status(),200);
  const avatarPackage=JSON.parse(await readFile(process.env.MEDIA_TEST_AVATAR_PACKAGE,'utf8'));
  assert.equal((await page.request.post(`${origin}/api/v1/admin/extensions`,{headers:{origin,'x-csrf-token':csrfToken},data:{...avatarPackage,publicKey:process.env.MEDIA_TEST_PUBLIC_KEY}})).status(),201);
  await page.reload();
  await panel.getByRole('button',{name:'Installées',exact:true}).click();
  await panel.getByRole('button',{name:'Paramètres de Avatars de test',exact:true}).click();
  await extension.getByRole('heading',{name:'Paramètres de Avatars de test'}).waitFor();
  assert.equal(await extension.getByRole('switch').count(),3);
  await extension.getByRole('button',{name:'Enregistrer les paramètres'}).click();
  await panel.getByText('Paramètres enregistrés.',{exact:true}).waitFor();
  const gallery=extension.getByRole('region',{name:'Collections d’avatars'});
  const png=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=','base64');
  await gallery.getByLabel('Images',{exact:true}).setInputFiles({name:'avatar.png',mimeType:'image/png',buffer:png});
  await gallery.getByRole('button',{name:'Ajouter les avatars',exact:true}).click();
  await gallery.getByRole('img',{name:'avatar.png'}).waitFor();
  await gallery.getByLabel('Nom de la collection',{exact:true}).last().fill('Ma galerie');
  await gallery.getByRole('button',{name:'Renommer',exact:true}).click();
  await gallery.getByText('Collection renommée.',{exact:true}).waitFor();
  await gallery.getByRole('button',{name:'Attribuer les avatars manquants maintenant'}).click();
  await gallery.getByText('1 compte a reçu un avatar.',{exact:true}).waitFor();
  const output = process.env.MEDIA_REPORT_DIR || '../../target/extensions-lifecycle-browser';
  await mkdir(output, { recursive: true });
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`${output}/avatar-gallery-desktop.png`,fullPage:true});
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`${output}/avatar-gallery-mobile.png`,fullPage:true});
  await gallery.getByRole('button',{name:'Supprimer avatar.png',exact:true}).click();
  await gallery.getByText('Cette collection est vide.',{exact:true}).waitFor();
  await page.setViewportSize({width:1440,height:1000});
  const playbackId='00000000-0000-4000-8000-000000000001';
  assert.equal((await page.request.put(`${origin}/api/v1/account/playback/${playbackId}`,{headers:{origin,'x-csrf-token':csrfToken},data:{workId:'work-films',position:20,duration:120,paused:false}})).status(),204);
  await page.route('**/api/v1/admin/libraries/films/scan',route=>route.fulfill({json:{status:'completed',discovered:400,probed:44,reused:0,errorsCount:356,errors:['Échec de probe synthétique : fixture incomplète.']}}));
  await page.goto(`${origin}/app/admin/libraries/`);
  await page.getByRole('heading',{name:'Vue d’ensemble',exact:true}).waitFor();
  await page.locator('#overview').getByRole('heading',{name:'Titre original',exact:true}).waitFor();
  await page.locator('#overview').getByText(/Films synthétiques · 356 erreur/).click();
  await page.getByText('Échec de probe synthétique : fixture incomplète.',{exact:true}).waitFor();
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`${output}/overview-desktop.png`,fullPage:true});
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`${output}/overview-mobile.png`,fullPage:true});
  await page.request.delete(`${origin}/api/v1/account/playback/${playbackId}`,{headers:{origin,'x-csrf-token':csrfToken}});
  for(const id of ['test.metadata','test.avatars'])assert.equal((await page.request.delete(`${origin}/api/v1/admin/extensions/${id}`,{headers:{origin,'x-csrf-token':csrfToken},data:{deleteData:false}})).status(),204);
  assert.deepEqual(errors, []);
  console.log('Signed extensions, dedicated settings, avatar upload/gallery/rename/delete/assignment and dashboard activity passed (desktop/mobile).');
} finally { await browser.close(); }

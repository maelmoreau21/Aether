import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Synthetic API scenario: proves client behavior, not real filesystem access.
const evidence = fileURLToPath(new URL('../../../docs/evidence/2026-10-25/library-editor/', import.meta.url));
await mkdir(evidence, { recursive: true });
const origin = process.env.MEDIA_EDITOR_TEST_ORIGIN ?? 'http://127.0.0.1:5187';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(15000);
  const errors = [], browsed = [];
  page.on('pageerror', error => errors.push(error.message));
  let theme = 'dark', saved;
  const library = { id: 'films', name: 'Films UHD', kind: 'films', nodeId: 'local', root: '/media/films', folders: [{ nodeId: 'local', root: '/media/films' }], enabled: true, userIds: [] };
  const empty = '/tmp/aether-core-empty-local-root';
  await page.route('**/api/v1/**', async route => {
    const req = route.request(), url = new URL(req.url()), path = url.pathname;
    let body = {};
    if (path.endsWith('/auth/me')) body = { user: { id: 'admin', login: 'admin', role: 'administrator', sso: false }, csrfToken: 'synthetic-csrf' };
    else if (path.endsWith('/preferences')) body = { theme, locale: 'fr' };
    else if (path.endsWith('/admin/filesystem/browse')) {
      const target = url.searchParams.get('path') ?? '';
      browsed.push(target);
      const directories = {
        '': { path: '', parent: null, directories: [{ name: '/', path: '/' }] },
        '/': { path: '/', parent: '', directories: [{ name: 'Films_UHD', path: '/Films_UHD' }, { name: 'tmp', path: '/tmp' }] },
        '/tmp': { path: '/tmp', parent: '/', directories: [{ name: 'aether-core-empty-local-root', path: empty }] },
        [empty]: { path: empty, parent: '/tmp', directories: [] },
        '/Films_UHD': { path: '/Films_UHD', parent: '/', directories: [{ name: 'Animation', path: '/Films_UHD/Animation' }] },
        '/Films_UHD/Animation': { path: '/Films_UHD/Animation', parent: '/Films_UHD', directories: [] },
        '/media/films': { path: '/media/films', parent: '/media', directories: [] }
      };
      body = directories[target];
      if (!body) { await route.fulfill({ status: 403, json: { error: 'Dossier non autorisé.' } }); return; }
    }
    else if (path.endsWith('/admin/libraries/films') && req.method() === 'PUT') {
      assert.equal(req.headers()['x-csrf-token'], 'synthetic-csrf');
      saved = req.postDataJSON(); Object.assign(library, saved); body = library;
    }
    else if (path.endsWith('/admin/libraries')) body = { libraries: [library] };
    else if (path.endsWith('/admin/nodes')) body = { nodes: [] };
    else if (path.endsWith('/admin/extensions')) body = { extensions: [{ manifest: { id: 'provider.metadata', name: 'Fournisseur synthétique' } }] };
    else if (path.endsWith('/admin/roots')) body = { roots: [empty] };
    else if (path.endsWith('/admin/users')) body = { users: [] };
    else if (path.endsWith('/admin/groups')) body = { groups: [] };
    else if (path.endsWith('/admin/oidc') || path.endsWith('/auth/oidc/status')) body = { enabled: false };
    else if (path.endsWith('/admin/api-keys')) body = { keys: [] };
    else if (path.endsWith('/admin/settings') || path.endsWith('/server')) body = { serverName: 'Aether', welcomeMessage: '' };
    else if (path.endsWith('/health')) body = { status: 'ok', mode: 'standalone' };
    else if (path.endsWith('/scan')) body = { status: 'idle', discovered: 0, probed: 0, reused: 0, errors: [] };
    await route.fulfill({ json: body });
  });
  const url = `${origin}/app/admin/libraries/#libraries-section`;
  await page.goto(url);
  await page.getByRole('button', { name: 'Gérer Films UHD', exact: true }).click().catch(async error => { console.error(await page.locator('body').innerText(), errors); throw error; });
  const editor = page.locator('dialog.library-editor');
  await editor.getByLabel('Dossier', { exact: true }).fill(empty);
  await editor.getByRole('button', { name: 'Parcourir…', exact: true }).click();
  const picker = page.locator('dialog.path-picker-dialog');
  await picker.getByText(/^Ce dossier ne contient aucun sous-dossier\./).waitFor();
  await picker.getByRole('button', { name: 'Dossier parent', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('#picker-path-input')?.value === '/tmp');
  await page.waitForTimeout(150);
  assert.equal(await picker.locator('#picker-path-input').inputValue(), '/tmp', 'navigation must not reset to initialPath');
  await picker.getByRole('button', { name: 'Racines / lecteurs', exact: true }).click();
  await picker.getByRole('button', { name: '/', exact: true }).click();
  await picker.getByRole('button', { name: 'Films_UHD', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('#picker-path-input')?.value === '/Films_UHD');
  await picker.getByRole('button', { name: 'Sélectionner ce dossier', exact: true }).waitFor();
  await picker.locator('#picker-path-input').fill('/not-authorized');
  await picker.locator('#picker-path-input').press('Enter');
  await picker.getByRole('alert').waitFor();
  assert.equal(await picker.getByRole('button', { name: 'Sélectionner ce dossier', exact: true }).isDisabled(), true);
  await picker.locator('#picker-path-input').fill('/Films_UHD/Animation');
  await picker.locator('#picker-path-input').press('Enter');
  await page.waitForFunction(() => document.querySelector('#picker-path-input')?.value === '/Films_UHD/Animation');
  await picker.getByRole('button', { name: 'Sélectionner ce dossier', exact: true }).click();
  await editor.getByRole('button', { name: 'Parcourir…', exact: true }).click();
  await picker.waitFor({ state: 'visible' });
  await picker.press('Escape');
  await picker.waitFor({ state: 'hidden' });
  assert.equal(await editor.getByLabel('Dossier', { exact: true }).inputValue(), '/Films_UHD/Animation');
  await editor.getByRole('button', { name: 'Ajouter ce dossier', exact: true }).click();
  assert.equal(await editor.getByText('/Films_UHD/Animation', { exact: true }).count(), 1);
  await editor.getByLabel('Langue préférée', { exact: true }).fill('fr-CA');
  await editor.getByLabel('Utiliser les fichiers NFO locaux', { exact: true }).uncheck();
  await editor.getByLabel('Actualiser les métadonnées locales', { exact: true }).selectOption('30');
  await editor.getByText('Images locales', { exact: true }).click();
  await editor.getByLabel('Logo transparent', { exact: true }).uncheck();
  await editor.getByLabel('Préférer les titres intégrés aux médias', { exact: true }).check();
  await editor.getByLabel('Sous-titres intégrés proposés par le lecteur', { exact: true }).selectOption('block_all');
  const providerGroups = [
    ['Métadonnées', 'metadata-providers', 'metadataProviders'],
    ['Images', 'image-providers', 'imageProviders'],
    ['Articles similaires', 'similar-providers', 'similarItemsProviders'],
    ['Segments média', 'segment-providers', 'segmentProviders'],
    ['Sous-titres', 'subtitle-providers', 'subtitleProviders']
  ];
  for (const [label, id] of providerGroups) {
    await editor.getByRole('button', { name: `Ajouter un fournisseur — ${label}`, exact: true }).click();
    await editor.locator(`#${id}-0`).fill(`provider.${id}`);
  }
  await editor.getByRole('button', { name: 'Ajouter un fournisseur — Métadonnées', exact: true }).click();
  await editor.locator('#metadata-providers-1').fill('provider.metadata-secondary');
  await editor.getByRole('checkbox', { name: 'Activer Métadonnées 2', exact: true }).uncheck();
  await editor.getByRole('button', { name: 'Monter Métadonnées 2', exact: true }).click();
  assert.equal(await editor.locator('#metadata-providers-0').inputValue(), 'provider.metadata-secondary');
  await editor.getByLabel('Actualiser les métadonnées depuis Internet', { exact: true }).selectOption('90');
  const preparedChecks = [
    ['Activer la surveillance en temps réel', 'realtimeMonitoring'],
    ['Ajouter automatiquement aux collections', 'automaticCollections'],
    ['Activer la détection d’intro', 'introDetectionEnabled'],
    ['Activer l’extraction d’images trickplay', 'trickplayEnabled'],
    ['Extraire les images trickplay pendant le scan', 'trickplayDuringScan'],
    ['Activer l’extraction des images de chapitres', 'chapterImagesEnabled'],
    ['Extraire les images des chapitres pendant le scan', 'chapterImagesDuringScan'],
    ['Activer le téléchargement automatique des sous-titres', 'subtitleDownloadEnabled'],
    ['Rechercher uniquement les langues correspondant à l’audio', 'subtitleMatchAudio'],
    ['Enregistrer les métadonnées au format NFO dans les médias', 'saveNfoToMedia'],
    ['Enregistrer les illustrations dans les dossiers médias', 'saveImagesToMedia'],
    ['Enregistrer les images trickplay à côté des médias', 'trickplaySaveToMedia'],
    ['Enregistrer les images de chapitres à côté des médias', 'chapterImagesSaveToMedia'],
    ['Enregistrer les sous-titres dans les dossiers médias', 'saveSubtitlesToMedia']
  ];
  for (const [label] of preparedChecks) await editor.getByLabel(label, { exact: true }).check();
  await editor.getByLabel('Intervalle entre les aperçus (secondes)', { exact: true }).fill('5');
  await editor.getByLabel('Largeur des aperçus (pixels)', { exact: true }).fill('640');
  const subtitleLanguages = editor.getByLabel('Langues des sous-titres (par ordre de préférence)', { exact: true });
  await subtitleLanguages.fill('');
  await subtitleLanguages.pressSequentially('fr-CA, en');
  assert.equal(await subtitleLanguages.inputValue(), 'fr-CA, en', 'typing a separator must preserve the unfinished second language');
  await subtitleLanguages.press('Tab');
  await editor.getByLabel('Exiger une correspondance exacte du fichier (hash)', { exact: true }).uncheck();
  for (const color of ['dark', 'light']) {
    theme = color;
    await page.evaluate(color => { document.documentElement.dataset.theme = color; }, color);
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 1000 });
      await editor.evaluate(el => { el.scrollTop = 0; });
      assert.equal(await editor.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true, `${color} ${width}: dialog overflow`);
      await page.screenshot({ path: `${evidence}/editor-${color}-${width}.png` });
      await editor.getByRole('heading', { name: 'Métadonnées et langue', exact: true }).scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${evidence}/options-${color}-${width}.png` });
      await editor.getByRole('heading', { name: 'Trickplay', exact: true }).scrollIntoViewIfNeeded();
      assert.equal(await editor.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true, `${color} ${width}: advanced controls overflow`);
      await page.screenshot({ path: `${evidence}/trickplay-${color}-${width}.png` });
      await editor.getByRole('heading', { name: 'Téléchargement des sous-titres', exact: true }).scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${evidence}/subtitles-${color}-${width}.png` });
    }
  }
  await editor.getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await editor.waitFor({ state: 'hidden' });
  assert.equal(saved.options.preferredMetadataLanguage, 'fr-CA');
  assert.equal(saved.options.readLocalNfo, false);
  assert.equal(saved.options.fetchLogo, false);
  assert.equal(saved.options.automaticRefreshIntervalDays, 30);
  assert.equal(saved.options.preferEmbeddedTitles, true);
  assert.equal(saved.options.embeddedSubtitlePolicy, 'block_all');
  assert.equal(saved.options.onlineRefreshIntervalDays, 90);
  assert.equal(saved.options.trickplayIntervalSeconds, 5);
  assert.equal(saved.options.trickplayWidth, 640);
  assert.deepEqual(saved.options.subtitleLanguages, ['fr-CA', 'en']);
  assert.equal(saved.options.subtitleRequireHash, false);
  assert.equal(saved.options.subtitleOnlyMissing, true);
  for (const [, key] of preparedChecks) assert.equal(saved.options[key], true, key);
  assert.deepEqual(saved.options.metadataProviders, [{ id: 'provider.metadata-secondary', enabled: false }, { id: 'provider.metadata-providers', enabled: true }]);
  for (const [, id, key] of providerGroups.slice(1)) assert.deepEqual(saved.options[key], [{ id: `provider.${id}`, enabled: true }]);
  assert.deepEqual(saved.folders, [{ nodeId: 'local', root: '/media/films' }, { nodeId: 'local', root: '/Films_UHD/Animation' }]);
  assert.ok(browsed.includes('/tmp'));
  await page.reload();
  await page.getByRole('button', { name: 'Gérer Films UHD', exact: true }).click();
  assert.equal(await editor.getByLabel('Intervalle entre les aperçus (secondes)', { exact: true }).inputValue(), '5');
  assert.equal(await editor.locator('#metadata-providers-0').inputValue(), 'provider.metadata-secondary');
  assert.equal(await editor.getByLabel('Activer l’extraction d’images trickplay', { exact: true }).isChecked(), true);
  assert.equal(await editor.getByLabel('Langues des sous-titres (par ordre de préférence)', { exact: true }).inputValue(), 'fr-CA, en');
  await editor.getByRole('button', { name: 'Annuler', exact: true }).click();
  assert.deepEqual(errors, []);
  console.log('PASS (synthetic API): directory navigation, advanced settings payload, provider priority, reload/reopen, dark/light desktop/mobile.');
} finally { await browser.close(); }


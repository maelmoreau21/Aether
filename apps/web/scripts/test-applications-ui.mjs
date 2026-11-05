import { chromium } from 'playwright';
import assert from 'node:assert/strict';

// Native UI contract with synthetic APIs: no Arr server, import source or media is modified.
const base = process.env.MEDIA_WEB_TEST_URL ?? 'http://127.0.0.1:5193/app';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage();
  const pageErrors = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  const timestamp = 1793800000;
  let capabilities = { requests: true, reports: true };
  let submitted;
  let reported;
  let importMappings;
  let configured;
  const records = [{ id: 'imported', kind: 'film', title: 'Historique conservé', externalId: '1', externalProvider: 'tmdb', serverId: 'radarr-1', status: 'suspended', sourceStatus: 'completed', createdAt: timestamp }];
  const servers = [{ id: 'radarr-1', name: 'Films NAS', kind: 'radarr', baseUrl: 'http://synthetic.invalid:7878', libraryId: 'films', rootFolder: '/movies', qualityProfileId: 1, isDefault: true, enabled: true }, { id: 'sonarr-2', name: 'Séries NAS', kind: 'sonarr', baseUrl: 'http://synthetic.invalid:8989', libraryId: 'series', rootFolder: '/series', qualityProfileId: 1, isDefault: true, enabled: true }];
  const film = { id: 'work-1', title: 'Film de test', kind: 'film', synthetic: false, libraryId: 'films', year: 2026, description: 'Synthetic metadata', editions: [] };
  await page.route('**/api/v1/**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const method = request.method();
    const input = request.postData() ? request.postDataJSON() : null;
    let json = {};
    if (path.endsWith('/auth/me')) json = { user: { id: 'admin', login: 'admin', role: 'administrator' }, csrfToken: 'synthetic' };
    else if (path.endsWith('/extensions/features')) json = capabilities;
    else if (path.endsWith('/server')) json = { name: 'Synthetic', serverName: 'Synthetic' };
    else if (path.endsWith('/preferences')) json = { theme: 'dark', locale: 'fr' };
    else if (path.endsWith('/libraries')) json = { libraries: [{ id: 'films', name: 'Films', kind: 'films' }, { id: 'series', name: 'Séries', kind: 'series' }] };
    else if (path.endsWith('/requests/destinations')) json = { servers };
    else if (path.endsWith('/requests/search')) json = { results: [{ kind: 'series', title: 'Une série', externalId: '7', externalProvider: 'tvdb', serverId: 'sonarr-2', year: 2026, summary: 'Découverte externe' }] };
    else if (path.endsWith('/requests') && method === 'POST') {
      const allowed = ['kind', 'title', 'externalId', 'externalProvider', 'serverId', 'seasons', 'episodes', 'albumIds', 'monitorFuture'];
      assert(Object.keys(input).every(key => allowed.includes(key)), 'Discovery metadata must not enter the strict admission body');
      assert.equal(request.headers()['x-csrf-token'], 'synthetic');
      submitted = input;
      const created = { ...input, id: 'new-request', status: 'pending', createdAt: timestamp };
      records.unshift(created); json = created;
    }
    else if (path.endsWith('/requests')) json = { requests: records, nextCursor: null };
    else if (/\/requests\/[^/]+\/action$/.test(path)) {
      const id = path.split('/').at(-2); const record = records.find(item => item.id === id);
      record.status = 'approved'; json = record;
    }
    else if (/\/requests\/[^/]+$/.test(path)) {
      const record = records.find(item => item.id === path.split('/').at(-1));
      json = { ...record, selection: { seasons: [2], episodes: [{ season: 2, episode: 3 }] }, events: [{ status: record.status, createdAt: timestamp }] };
    }
    else if (path.endsWith('/admin/acquisition/servers') && method === 'POST') {
      configured = input;
      const saved = { ...input, id: 'radarr-new', hasApiKey: true };
      delete saved.apiKey; servers.push(saved); json = { server: saved };
    }
    else if (path.endsWith('/admin/acquisition/servers/radarr-new') && method === 'PUT') {
      configured = input;
      const saved = { ...input, id: 'radarr-new', hasApiKey: true };
      delete saved.apiKey;
      Object.assign(servers.find(server => server.id === saved.id), saved);
      json = { server: saved };
    }
    else if (path.endsWith('/admin/acquisition/servers/radarr-new/options')) json = { rootFolders: [{ path: '/movies' }], qualityProfiles: [{ id: 1, name: 'Qualité test' }], metadataProfiles: [] };
    else if (path.endsWith('/admin/acquisition/servers')) json = { servers };
    else if (path.endsWith('/admin/acquisition/settings')) json = { autoApprove: false, maxPendingPerUser: 20 };
    else if (path.endsWith('/admin/acquisition/locations')) json = { locations: [{ id: 'films', name: 'Films', kind: 'films', nodeId: 'node-1', nodeName: 'NAS', root: '/media/movies' }, { id: 'series', name: 'Séries', kind: 'series', nodeId: 'node-2', nodeName: 'NAS 2', root: '/media/series' }] };
    else if (path.endsWith('/admin/users')) json = { users: [{ id: 'admin', login: 'admin' }] };
    else if (path.endsWith('/admin/imports/preview')) json = { id: 'import-1', sourceUrl: 'http://synthetic.invalid/', users: [{ id: '12', name: 'Ancien auteur' }], servers: [{ id: 'radarr:1', name: 'Ancien Radarr', kind: 'radarr' }], counts: { requests: 1, reports: 1 }, warnings: [] };
    else if (path.endsWith('/admin/imports/import-1/start')) { importMappings = input; json = { id: 'import-1', status: 'running' }; }
    else if (path.endsWith('/admin/imports')) json = { imports: [] };
    else if (path.endsWith('/reports') && method === 'POST') { reported = input; json = { id: 'report-1', ...input, status: 'open', createdAt: timestamp }; }
    else if (path.endsWith('/reports')) json = { reports: [{ id: 'report-1', category: 'audio', description: 'Audio incorrect', status: 'resolved', positionSeconds: null, endSeconds: null, createdAt: timestamp }] };
    else if (path.endsWith('/reports/report-1')) json = { id: 'report-1', category: 'audio', description: 'Audio incorrect', status: 'resolved', positionSeconds: null, endSeconds: null, comments: [{ message: 'Correction effectuée', authorLabel: 'Auteur historique', createdAt: timestamp }], createdAt: timestamp };
    else if (path.endsWith('/catalog/work-1')) json = film;
    else if (path.endsWith('/catalog')) json = { items: [], total: 0 };
    else if (path.endsWith('/progress')) json = { items: [] };
    else if (path.endsWith('/lists')) json = { lists: [] };
    await route.fulfill({ json });
  });

  await page.goto(`${base}/requests/`);
  await page.getByRole('heading', { name: 'Trouver un contenu', exact: true }).waitFor();
  await page.getByRole('combobox').first().selectOption('series');
  await page.getByLabel('Votre recherche').fill('Une série');
  await page.getByRole('button', { name: 'Rechercher', exact: true }).click();
  await page.getByRole('button', { name: 'Demander', exact: true }).click();
  await page.getByLabel('Saisons (facultatif)').fill('2');
  await page.getByLabel('Épisodes (facultatif)').fill('2:3');
  await page.getByRole('button', { name: 'Confirmer la demande' }).click();
  await page.getByRole('status').filter({ hasText: 'Demande enregistrée' }).waitFor();
  assert.deepEqual(submitted.seasons, [2]);
  assert.deepEqual(submitted.episodes, [{ season: 2, episode: 3 }]);
  assert.equal(submitted.serverId, 'sonarr-2');
  const created = page.locator('.record').filter({ has: page.getByRole('heading', { name: 'Une série', exact: true }) });
  await created.getByRole('button', { name: 'Voir le suivi' }).click();
  await created.getByText('Saisons : 2', { exact: true }).waitFor();
  await created.getByText('Épisodes : 2:3', { exact: true }).waitFor();
  const imported = page.locator('.record').filter({ hasText: 'Historique conservé' });
  await imported.getByRole('button', { name: 'Reprendre la demande', exact: true }).click();
  await imported.getByText('Approuvée', { exact: true }).waitFor();

  await page.goto(`${base}/requests/admin/`);
  await page.getByRole('heading', { name: 'Demandes et acquisitions' }).waitFor();
  await page.getByLabel('Nom', { exact: true }).fill('Nouveau NAS');
  await page.getByLabel('Adresse du serveur', { exact: true }).fill('http://synthetic.invalid:7878');
  await page.getByLabel('Clé API', { exact: true }).fill('synthetic-arr-key');
  await page.getByLabel('Emplacement de destination').selectOption('films');
  await page.getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await page.getByRole('button', { name: 'Charger les dossiers et profils' }).click();
  await page.getByLabel('Dossier distant').selectOption('/movies');
  await page.getByLabel('Profil de qualité').selectOption('1');
  await page.getByLabel('Connexion activée').check();
  await page.getByRole('button', { name: 'Enregistrer', exact: true }).click();
  await page.getByRole('status').filter({ hasText: 'Connexion enregistrée' }).waitFor();
  assert.equal(configured.libraryId, 'films');
  assert.equal(configured.rootFolder, '/movies');
  assert.equal(configured.enabled, true);
  assert(!JSON.stringify(servers).includes('synthetic-arr-key'));
  await page.getByLabel('Adresse de l’application précédente').fill('http://synthetic.invalid');
  await page.getByLabel('Clé API administrateur de la source').fill('synthetic-only');
  await page.getByRole('button', { name: 'Analyser sans importer' }).click();
  await page.getByLabel('Ancien auteur').selectOption('admin');
  await page.getByLabel('Ancien Radarr · radarr').selectOption('radarr-1');
  await page.getByLabel('J’ai vérifié les correspondances.', { exact: false }).check();
  await Promise.all([
    page.waitForResponse(response => response.url().endsWith('/admin/imports')),
    page.getByRole('button', { name: 'Importer les dossiers', exact: true }).click()
  ]);
  assert.deepEqual(importMappings, { userMappings: { 12: 'admin' }, serverMappings: { 'radarr:1': 'radarr-1' } });

  await page.goto(`${base}/films/?work=work-1`);
  await page.getByRole('button', { name: 'Signaler', exact: true }).click();
  await page.getByLabel('Description', { exact: true }).fill('Audio incorrect sur cette copie');
  await page.getByRole('button', { name: 'Envoyer le signalement' }).click();
  await page.getByRole('status').filter({ hasText: 'Signalement envoyé' }).waitFor();
  assert.equal(reported.workId, 'work-1');
  assert(!JSON.stringify(reported).includes('synthetic-only'));
  await page.getByRole('dialog').getByRole('button', { name: 'Fermer', exact: true }).click();

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`${base}/reports/`);
  await page.getByRole('button', { name: 'Ouvrir le suivi' }).click();
  await page.getByText('Auteur historique ·', { exact: false }).waitFor();
  assert(!await page.getByText('Position : null s').count());
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));

  capabilities = { requests: false, reports: false };
  await page.goto(`${base}/requests/`);
  await page.getByText('L’extension officielle Demandes doit être installée', { exact: false }).waitFor();
  assert.equal(await page.getByRole('heading', { name: 'Trouver un contenu' }).count(), 0);
  assert.deepEqual(pageErrors, []);
  console.log('Applications UI: search/admission scope, detail events, suspended restart, physical destination setup, migration mappings, contextual report, mobile and disabled gates passed.');
} finally { await browser.close(); }

import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Synthetic API contract/layout checks; the real API scenario is test-admin-live.mjs.
const evidence = fileURLToPath(new URL('../../../docs/evidence/2026-10-19/settings-jellyfin/', import.meta.url));
await mkdir(evidence, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage(); page.setDefaultTimeout(10000);
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  let role = 'administrator', theme = 'dark', launches = 0, backupLaunches = 0, failSave = false, failRead = false, available = true;
  let preferences = { theme, locale: 'fr' }, plans = [];
  const library = { id: 'films', name: 'Films de test', kind: 'films', nodeId: 'local', root: '/media/films', enabled: true, userIds: [] };
  const archive = { available: true, running: false, archives: [], reason: null, schedule: { enabled: false, intervalMinutes: 1440, nextDueAt: 1792447200, lastStartedAt: null, lastCompletedAt: null, lastOutcome: 'never' } };
  await page.route('**/api/v1/**', async route => {
    const req = route.request(), path = new URL(req.url()).pathname, method = req.method(); let body;
    if (['PUT', 'POST', 'DELETE'].includes(method)) assert.equal(req.headers()['x-csrf-token'], 'synthetic-csrf');
    if (path.endsWith('/auth/me')) body = { user: { id: 'admin', login: 'admin', role, sso: false }, csrfToken: 'synthetic-csrf' };
    else if (path.endsWith('/preferences')) {
      if (method === 'PUT') { preferences = { ...preferences, ...req.postDataJSON() }; await route.fulfill({ status: 204 }); return; }
      body = { ...preferences, theme };
    }
    else if (path.endsWith('/admin/tasks/schedules/films')) {
      if (failSave) { await route.fulfill({ status: 403 }); return; }
      if (method === 'DELETE') plans = [];
      else { const data = req.postDataJSON(); plans = [{ libraryId: 'films', ...data, enabled: data.enabled ? 1 : 0, nextRunAt: 1792447200, lastRunAt: null, lastOutcome: 'never' }]; }
      await route.fulfill({ status: 204 }); return;
    }
    else if (path.endsWith('/admin/tasks/schedules')) { if (failRead) { await route.fulfill({ status: 503 }); return; } body = plans; }
    else if (path.endsWith('/admin/backups/schedule')) { Object.assign(archive.schedule, req.postDataJSON()); await route.fulfill({ status: 204 }); return; }
    else if (path.endsWith('/admin/backups')) { if (method === 'POST') backupLaunches++; body = { ...archive, available }; }
    else if (path.endsWith('/admin/libraries/films/scan')) { if (method === 'POST') launches++; body = { status: 'completed', discovered: 2, probed: 1, reused: 1, errors: [], errorsCount: 0 }; }
    else if (path.endsWith('/admin/libraries')) body = { libraries: [library] };
    else if (path.endsWith('/admin/users')) body = { users: [{ id: 'admin', login: 'admin', role: 'administrator', sso: false }] };
    else if (path.endsWith('/admin/groups')) body = { groups: [] };
    else if (path.endsWith('/admin/api-keys')) body = { keys: [] };
    else if (path.endsWith('/admin/nodes')) body = { nodes: [] };
    else if (path.endsWith('/admin/roots')) body = { roots: ['/media'] };
    else if (path.endsWith('/admin/oidc') || path.endsWith('/auth/oidc/status')) body = { enabled: false };
    else if (path.endsWith('/health')) body = { status: 'ok', mode: 'standalone' };
    else body = { name: 'Aether' };
    await route.fulfill({ json: body });
  });
  const url = 'http://127.0.0.1:5187/app/admin/libraries/';
  async function goTask(id) { await page.goto(`${url}#tasks-section${id ? `?task=${id}` : ''}`); await page.locator('#tasks-section').waitFor({ state: 'visible' }); }
  async function nav(id) {
    const toggle = page.locator('.mobile-menu-toggle'); if (await toggle.isVisible() && await toggle.getAttribute('aria-expanded') === 'false') await toggle.click();
    await page.locator(`.admin-sidebar a[href$="#${id}-section"]`).click(); await page.locator(`#${id}-section`).waitFor({ state: 'visible' });
  }
  async function noOverflow(context) { assert.equal(await page.locator('main').evaluate(el => el.scrollWidth > el.clientWidth + 1), false, context); }
  async function saveInterval(value, unit) {
    await page.getByRole('button', { name: /Ajouter un déclencheur|Modifier le déclencheur/ }).click();
    await page.getByRole('dialog').getByLabel('Toutes les', { exact: true }).fill(value);
    await page.getByRole('dialog').getByLabel('Unité', { exact: true }).selectOption(unit);
    await page.getByRole('dialog').getByRole('button', { name: 'Enregistrer', exact: true }).click();
  }
  await goTask();
  assert.equal(await page.locator('.task-row').count(), 2);
  await page.getByRole('button', { name: 'Scanner Films de test', exact: true }).click(); assert.equal(launches, 1);
  await page.getByRole('link', { name: 'Configurer Indexer Films de test' }).click();
  assert.match(page.url(), /task=scan-films/);
  await saveInterval('1', '60'); await page.getByText('Déclencheur enregistré.', { exact: true }).waitFor(); assert.equal(plans[0].intervalMinutes, 60);
  await page.reload(); await page.getByText('Chaque heure', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Supprimer le déclencheur' }).click(); await page.getByText(/Aucun déclencheur actif/).waitFor(); assert.equal(plans.length, 0);
  failSave = true; await saveInterval('24', '60'); await page.getByRole('dialog').getByRole('alert').waitFor(); assert.equal(plans.length, 0);
  await page.getByRole('button', { name: 'Annuler', exact: true }).click(); failSave = false;
  await saveInterval('1', '1'); await page.getByText('Intervalle invalide. Vérifiez les limites indiquées.').waitFor(); assert.equal(plans.length, 0);
  await page.getByRole('button', { name: 'Annuler', exact: true }).click();
  await saveInterval('24', '60'); await page.getByText('Déclencheur enregistré.', { exact: true }).waitFor();
  await page.getByRole('link', { name: 'Retour aux tâches' }).click(); await page.goBack(); await page.getByText('Chaque jour', { exact: true }).waitFor();
  await goTask('backup'); await saveInterval('2', '60'); await page.getByText('Déclencheur enregistré.', { exact: true }).waitFor(); assert.equal(archive.schedule.intervalMinutes, 120);
  await page.getByRole('button', { name: 'Supprimer le déclencheur' }).click(); await page.getByText(/Aucun déclencheur actif/).waitFor(); assert.equal(archive.schedule.enabled, false);
  await page.getByRole('button', { name: 'Exécuter Sauvegarder le serveur' }).click(); await page.getByText('Sauvegarde créée.', { exact: true }).waitFor(); assert.equal(backupLaunches, 1);
  available = false; await goTask(); await page.reload(); assert.equal(await page.getByRole('button', { name: 'Créer une sauvegarde', exact: true }).isDisabled(), true); available = true;
  failRead = true; await page.getByRole('button', { name: 'Actualiser', exact: true }).click(); await page.getByText(/planifications des bibliothèques sont indisponibles/).waitFor(); failRead = false;
  await page.getByRole('button', { name: 'Actualiser', exact: true }).click(); await page.getByText(/planifications des bibliothèques sont indisponibles/).waitFor({ state: 'hidden' });
  for (const selectedTheme of ['light', 'dark']) {
    theme = selectedTheme;
    await goTask(); await page.reload(); await page.locator('.task-row').first().waitFor();
    assert.equal(await page.locator('html').getAttribute('data-theme'), theme);
    for (const width of [360, 390, 768, 1024, 1440]) {
      await page.setViewportSize({ width, height: 1000 }); await goTask(); await noOverflow(`${theme}/${width}/list`);
      if ([390, 1440].includes(width)) await page.screenshot({ path: `${evidence}/tasks-${theme}-${width}.png`, fullPage: true, animations: 'disabled' });
      await page.getByRole('link', { name: 'Configurer Indexer Films de test' }).click(); await page.getByText('Chaque jour', { exact: true }).waitFor(); await noOverflow(`${theme}/${width}/detail`);
      if ([390, 1440].includes(width)) await page.screenshot({ path: `${evidence}/trigger-${theme}-${width}.png`, fullPage: true, animations: 'disabled' });
    }
    for (const id of ['users', 'groups', 'libraries', 'nodes', 'sso', 'backups', 'api-keys', 'extensions']) { await nav(id); await noOverflow(`${theme}/${id}`); }
    for (const width of [360, 1440]) {
      await page.setViewportSize({ width, height: 1000 }); await page.goto('http://127.0.0.1:5187/app/settings/#account');
      for (const id of ['account', 'appearance', 'playback', 'sso']) {
        await page.locator(`.settings-nav a[href$="#${id}"]`).click(); await page.locator(`.settings-nav a[href$="#${id}"][aria-current="page"]`).waitFor(); await noOverflow(`${theme}/${width}/${id}`);
      }
      await page.locator('.settings-nav a[href$="#playback"]').click();
      await page.getByRole('heading', { name: 'Lecture', exact: true }).waitFor();
      await page.screenshot({ path: `${evidence}/settings-${theme}-${width}.png`, fullPage: true, animations: 'disabled' });
    }
  }
  await page.getByLabel('Décalage audio (secondes)', { exact: true }).fill('1.5');
  await page.getByLabel('Décalage des sous-titres (secondes)', { exact: true }).fill('-2');
  const offsetSaved = page.waitForResponse(r => r.url().endsWith('/preferences') && r.request().method() === 'PUT');
  await page.getByRole('button', { name: 'Enregistrer les décalages' }).click();
  await offsetSaved;
  assert.deepEqual(preferences.offsets, { audio: 1.5, subtitle: -2 });
  await page.reload(); assert.equal(await page.getByLabel('Décalage audio (secondes)', { exact: true }).inputValue(), '1.5');
  library.name = 'Bibliothèque_Cinéma_Archives_Françaises_'.repeat(6);
  await page.setViewportSize({ width: 360, height: 1000 }); await goTask(); await page.reload(); await noOverflow('long library name');
  await page.locator('.task-link').first().focus(); await page.keyboard.press('Enter'); await page.locator('.trigger-list').waitFor(); await noOverflow('long detail name');
  await goTask('missing'); await page.getByRole('heading', { name: 'Tâche introuvable' }).waitFor();
  role = 'user'; await page.reload(); await page.getByText('Accès refusé', { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log('PASS: task list/detail/history/direct URLs; scan/backup trigger add/delete, errors, limits, CSRF; 2 themes × 5 widths; settings tabs/offset persistence; long names and role denial (mock API).');
} finally { await browser.close(); }

import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// UI contract and responsive layout only. No real scheduler, media or Node.
const origin = process.env.MEDIA_TEST_ORIGIN || 'http://127.0.0.1:5189';
const evidence = fileURLToPath(new URL('../../../docs/evidence/2026-10-24/distributed-tasks/ui/', import.meta.url));
await mkdir(evidence, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage();
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  const actions = [];
  const submissions = []; let detailReads = 0;
  let theme = 'dark';
  const library = { id: 'films', name: 'Archives des films', kind: 'films', nodeId: 'local', root: '/read-only', enabled: true, userIds: [] };
  let jobs = [{ id: 'synthetic-job', libraryId: 'films', owner: 'synthetic-admin', nodeId: 'local', status: 'running', category: 'scan', priority: 100, attempt: 1, maxAttempts: 3, progress: 0, processedItems: 12, createdAt: 1792860000, updatedAt: 1792860001, startedAt: 1792860000, finishedAt: null, leaseUntil: 1792860300, error: '' }];
  await page.route('**/api/v1/**', async route => {
    const request = route.request(); const path = new URL(request.url()).pathname; const method = request.method();
    let body;
    if (['POST', 'PUT', 'DELETE'].includes(method)) assert.equal(request.headers()['x-csrf-token'], 'synthetic-csrf');
    if (path.endsWith('/auth/me')) body = { user: { id: 'synthetic-admin', login: 'admin', role: 'administrator', sso: false }, csrfToken: 'synthetic-csrf' };
    else if (path.endsWith('/preferences')) body = { theme, locale: 'fr' };
    else if (path.endsWith('/admin/tasks/jobs')) {
      if (method === 'POST') {
        const input = request.postDataJSON(); assert.equal(input.libraryId, 'films'); assert.match(input.idempotencyKey, /^[0-9a-f]{32}$/);
        submissions.push(input);
        jobs = [{ ...jobs[0], id: 'new-job', category: input.category, targetId: input.targetId ?? '', status: 'queued', attempt: 0, startedAt: null, leaseUntil: null, progress: 0, processedItems: 0, resultJson: '{}' }];
        await route.fulfill({ status: 202, json: jobs[0] }); return;
      }
      body = { jobs, limit: 50, offset: 0 };
    }
    else if (path.endsWith('/admin/tasks/jobs/new-job') && method === 'GET') {
      detailReads++; body = { ...jobs[0], resultJson: JSON.stringify({ width: 320, message: '<script>throw new Error("must remain text")</script>' }) };
    }
    else if (/\/admin\/tasks\/jobs\/[^/]+\/(pause|resume|cancel)$/.test(path)) {
      const action = path.split('/').at(-1); actions.push(action);
      jobs[0] = { ...jobs[0], status: action === 'pause' ? 'paused' : action === 'resume' ? 'queued' : 'cancelled', leaseUntil: null };
      await route.fulfill({ status: 204 }); return;
    }
    else if (path.endsWith('/admin/tasks/schedules')) body = [];
    else if (path.endsWith('/admin/backups')) body = { available: false, running: false, schedule: { enabled: false, intervalMinutes: 1440, nextDueAt: 0, lastStartedAt: null, lastCompletedAt: null, lastOutcome: 'never' } };
    else if (path.endsWith('/admin/libraries')) body = { libraries: [library] };
    else if (path.endsWith('/admin/users')) body = { users: [{ id: 'synthetic-admin', login: 'admin', role: 'administrator', sso: false }] };
    else if (path.endsWith('/admin/nodes')) body = { nodes: [] };
    else if (path.endsWith('/admin/roots')) body = { roots: ['/read-only'] };
    else if (path.endsWith('/admin/groups')) body = { groups: [] };
    else if (path.endsWith('/admin/api-keys')) body = { keys: [] };
    else if (path.endsWith('/scan')) body = { status: 'completed', discovered: 1, probed: 1, reused: 0, errors: [], errorsCount: 0 };
    else if (path.endsWith('/account/avatar')) { await route.fulfill({ status: 404 }); return; }
    else if (path.endsWith('/health')) body = { status: 'ok', mode: 'standalone' };
    else if (path.includes('/oidc')) body = { enabled: false };
    else body = { serverName: 'Aether', name: 'Aether' };
    await route.fulfill({ json: body });
  });
  await page.goto(`${origin}/app/admin/libraries/#tasks-section`);
  const queue = page.getByRole('region', { name: 'File de tâches distribuées' });
  await queue.getByText('12 éléments traités · Total encore inconnu', { exact: true }).waitFor();
  assert.equal(await queue.locator('progress').count(), 0, 'unknown scan total must not produce a percentage');
  await queue.getByRole('button', { name: 'Suspendre', exact: true }).click();
  await queue.getByRole('button', { name: 'Reprendre', exact: true }).waitFor();
  await queue.getByRole('button', { name: 'Reprendre', exact: true }).click();
  await queue.getByText('En attente', { exact: true }).waitFor();
  await queue.getByRole('button', { name: 'Annuler', exact: true }).click();
  await queue.getByText('Annulée', { exact: true }).waitFor();
  assert.deepEqual(actions, ['pause', 'resume', 'cancel']);
  await queue.getByLabel('Bibliothèque cible', { exact: true }).selectOption('films');
  await queue.getByRole('button', { name: 'Planifier un scan', exact: true }).click();
  await queue.getByText('Scan ajouté à la file persistante.', { exact: true }).waitFor();
  await queue.getByLabel('Type de tâche', { exact: true }).selectOption('prepare');
  await queue.getByLabel('Composant à préparer', { exact: true }).fill('audio-1');
  await queue.getByLabel('Type de tâche', { exact: true }).selectOption('image');
  assert.equal(await queue.getByRole('button', { name: 'Planifier la tâche', exact: true }).isDisabled(), true);
  await queue.getByLabel('Identifiant cible', { exact: true }).fill('synthetic-image-id');
  await queue.getByLabel('Largeur de l’image', { exact: true }).selectOption('320');
  await queue.getByRole('button', { name: 'Planifier la tâche', exact: true }).click();
  await queue.getByText('Tâche ajoutée à la file persistante.', { exact: true }).waitFor();
  assert.deepEqual({ category: submissions[1].category, targetId: submissions[1].targetId, width: submissions[1].width, component: submissions[1].component }, { category: 'image', targetId: 'synthetic-image-id', width: 320, component: undefined });
  assert.equal(detailReads, 0, 'results must not be loaded with the job list');
  await queue.getByText('Résultat du traitement', { exact: true }).click();
  await queue.locator('pre').getByText(/must remain text/).waitFor();
  assert.equal(detailReads, 1);
  assert.equal(await queue.locator('script').count(), 0, 'result JSON must remain escaped text');
  for (const selectedTheme of ['light', 'dark']) {
    theme = selectedTheme; await page.reload(); await queue.getByText('En attente', { exact: true }).waitFor();
    for (const width of [390, 1440]) {
      await page.setViewportSize({ width, height: 1000 });
      await queue.scrollIntoViewIfNeeded();
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `overflow ${theme}/${width}`);
      await page.screenshot({ path: `${evidence}/queue-${theme}-${width}.png`, fullPage: true, animations: 'disabled' });
    }
  }
  assert.deepEqual(errors, []);
  await writeFile(`${evidence}/result.json`, JSON.stringify({ browser: browser.version(), mockApi: true, actions, categories: submissions.map(input => input.category), onDemandResultReads: detailReads, widths: [390, 1440], themes: ['light', 'dark'], pageErrors: errors }, null, 2));
  console.log('PASS: mock queue rendering, scan/image enqueue, required target, widths, pause/resume/cancel, CSRF, on-demand result JSON escaped as text; light/dark at390/1440 without horizontal overflow; no browser runtime errors.');
} finally { await browser.close(); }

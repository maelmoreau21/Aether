import { chromium } from 'playwright';
import assert from 'node:assert/strict';

// UI contract test with synthetic API responses; does not validate backend RBAC.
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  let role = 'administrator';
  let launches = 0;
  const library = { id: 'films', name: 'Films de test', kind: 'films', nodeId: 'local', root: '/media/films', enabled: true, userIds: ['user'] };
  await page.route('**/api/v1/**', async route => {
    const path = new URL(route.request().url()).pathname;
    let body;
    if (path.endsWith('/auth/me')) body = { user: { id: 'admin', login: 'admin', role }, csrfToken: 'synthetic-csrf' };
    else if (path.endsWith('/admin/libraries/films/scan')) {
      if (route.request().method() === 'POST') {
        assert.equal(route.request().headers()['x-csrf-token'], 'synthetic-csrf');
        launches++;
      }
      body = { status: 'completed', discovered: 2, probed: 1, reused: 1, errors: [], errorsCount: 0 };
    }
    else if (path.endsWith('/preferences')) body = {};
    else if (path.endsWith('/admin/groups')) body = { groups: [] };
    else if (path.endsWith('/admin/api-keys')) body = { keys: [] };
    else if (path.endsWith('/admin/tasks/schedules')) body = [];
    else if (path.endsWith('/admin/backups')) body = { available: false, archives: [], running: false, reason: 'Synthetic test' };
    else if (path.endsWith('/admin/libraries')) body = { libraries: [library] };
    else if (path.endsWith('/admin/users')) body = { users: [{ id: 'admin', login: 'admin', role: 'administrator', sso: false }, { id: 'user', login: 'alice', role: 'user', sso: false }] };
    else if (path.endsWith('/admin/roots')) body = { roots: ['/media'] };
    else if (path.endsWith('/admin/nodes')) body = { nodes: [] };
    else if (path.endsWith('/admin/oidc')) body = { enabled: false };
    else if (path.endsWith('/health')) body = { status: 'ok', mode: 'standalone' };
    else body = { name: 'Aether' };
    await route.fulfill({ json: body });
  });
  await page.goto('http://127.0.0.1:5187/app/admin/libraries/#tasks-section');
  await page.getByRole('button', { name: 'Scanner Films de test', exact: true }).click();
  assert.equal(launches, 1);
  for (const id of ['users', 'backups', 'groups', 'api-keys', 'extensions', 'libraries', 'nodes', 'sso']) {
    await page.locator(`.admin-sidebar a[href$="#${id}-section"]`).click();
    await page.locator(`#${id}-section`).waitFor({ state: 'visible' });
    assert.equal(await page.locator('#tasks-section').isVisible(), false);
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('.admin-sidebar a[href$="#tasks-section"]').click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: '../../docs/evidence/admin-console-mobile.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({ path: '../../docs/evidence/admin-console-desktop.png', fullPage: true });
  role = 'user';
  await page.reload();
  await page.getByRole('heading', { name: 'Tâches', exact: true }).waitFor({ state: 'hidden' });
  await page.getByText('Accès refusé', { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log('Admin UI: navigation, direct hash, CSRF scan request, mobile width, user denial and JS checks passed (mock API).');
} finally {
  await browser.close();
}

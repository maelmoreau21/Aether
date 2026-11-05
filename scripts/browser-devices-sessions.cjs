// Production Svelte views with synthetic session APIs, including an SSO account.
// Run against the built Web preview; this does not validate a deployed SSO provider.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5187/app/';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/devices-sessions';
fs.mkdirSync(out, { recursive: true });

async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { browser: browser.version(), syntheticApis: true, scenarios: [] };
  try {
    for (const theme of ['dark', 'light']) for (const width of [390, 1440]) {
      const { page, context, errors } = await fixturePage(browser, width, theme);
      const now = Math.floor(Date.now() / 1000);
      let sessions = [
        { id: 'remote', device: 'Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 Chrome/131.0.0.0 Mobile Safari/537.36', createdAt: now - 86400, lastSeenAt: now - 200, expiresAt: now + 864000, current: false },
        { id: 'current', device: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36', createdAt: now - 3600, lastSeenAt: now, expiresAt: now + 2500000, current: true }
      ];
      let failLoad = false, failRevoke = false, deletes = 0;
      await page.route('**/api/v1/auth/me', route => route.fulfill({ json: { user: { id: 'visual', login: 'visual-sso', role: 'user', sso: true }, csrfToken: 'synthetic-test' } }));
      await page.route('**/api/v1/extensions/features', route => route.fulfill({ json: { requests: true, reports: true } }));
      await page.route('**/api/v1/account/sessions**', async route => {
        const request = route.request();
        if (request.method() === 'DELETE') {
          deletes += 1;
          assert.equal(request.headers()['x-csrf-token'], 'synthetic-test');
          if (failRevoke) return route.fulfill({ status: 503, json: { error: 'unavailable' } });
          const id = new URL(request.url()).pathname.split('/').pop();
          sessions = id === 'sessions' ? [] : sessions.filter(item => item.id !== id);
          return route.fulfill({ status: 204 });
        }
        if (failLoad) return route.fulfill({ status: 503, json: {} });
        return route.fulfill({ json: { sessions } });
      });
      await page.goto(base + 'settings/#devices');
      const region = page.getByRole('region', { name: /^Vos sessions/ });
      await region.getByText('Cet appareil', { exact: true }).waitFor();
      assert.match(await region.locator('article').first().textContent(), /Chrome · Windows/);
      assert.equal(await region.locator('article').count(), 2);
      assert.equal(await page.locator('.desktop-nav').getByRole('link', { name: 'Demandes', exact: true }).count(), 0);
      await page.getByRole('link', { name: 'Demandes', exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await region.locator('article').first().getByText('Détails du navigateur').click();
      await region.locator('article').first().getByText(/Mozilla\/5.0/).waitFor();
      await region.locator('article').first().getByText('Détails du navigateur').click();
      await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo(0, 0); });
      await page.screenshot({ path: `${out}/${theme}-${width}.png`, fullPage: true });

      failLoad = true;
      await region.getByRole('button', { name: 'Actualiser', exact: true }).click();
      await region.getByRole('alert').waitFor();
      assert.equal(await region.locator('article').count(), 2, 'A failed refresh preserves the known sessions');
      failLoad = false;
      await region.getByRole('button', { name: 'Actualiser', exact: true }).click();
      await region.getByRole('alert').waitFor({ state: 'hidden' });

      failRevoke = true;
      await region.getByRole('button', { name: 'Déconnecter Chrome · Android', exact: true }).click();
      await region.getByRole('alert').waitFor();
      assert.equal(await region.locator('article').count(), 2, 'A failed revocation preserves the session');
      failRevoke = false;
      await region.getByRole('button', { name: 'Déconnecter Chrome · Android', exact: true }).click();
      await region.getByText('La session a été déconnectée.', { exact: true }).waitFor();
      await page.waitForFunction(() => document.querySelectorAll('.session-list article').length === 1);
      assert.equal(await region.locator('.session-count').textContent(), '1');
      const beforeConfirm = deletes;
      await region.getByRole('button', { name: 'Déconnecter tous les appareils', exact: true }).click();
      await region.getByRole('button', { name: 'Annuler', exact: true }).click();
      assert.equal(deletes, beforeConfirm, 'Cancelling the global confirmation does not revoke anything');
      await region.getByRole('button', { name: 'Déconnecter tous les appareils', exact: true }).click();
      await region.getByRole('button', { name: 'Confirmer la déconnexion', exact: true }).click();
      await region.getByText('Aucune session à afficher', { exact: true }).waitFor();
      assert.equal(deletes, beforeConfirm + 1);
      assert.equal(await region.locator('article').count(), 0);
      assert.deepEqual(errors, []);
      report.scenarios.push({ theme, width, ssoAccount: true, readableDeviceLabels: true, currentFirst: true, refreshFailureRecovery: true, revokeFailureRecovery: true, remoteRevoked: true, confirmAll: true, emptyState: true, headerRequestsRemoved: true, settingsRequestsPresent: true, overflow: false });
      await context.close();
    }
    report.passed = true;
  } catch (error) {
    report.failure = String(error.stack || error);
    throw error;
  } finally {
    await browser.close();
    fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  }
}
run().catch(error => { console.error(error); process.exitCode = 1; });

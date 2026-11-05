// Real Svelte UI with synthetic APIs. Rust route tests cover the actual Core protocol.
// SSO navigation below is simulated; no live IdP or physical camera is exercised.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
// Generated using the same Rust qrcode 0.14.1 encoder as Core; public synthetic URL.
const qr = require('./fixtures/quick-connect-qr.json');
const qrModules = qr.rows.flatMap(row => [...row].map(value => value === '1'));
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5187/app/';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/quick-connect';
fs.mkdirSync(out, { recursive: true });
const identity = { user: { id: 'visual', login: 'sso-member', role: 'user', sso: true }, csrfToken: 'synthetic-device-csrf' };
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { browser: browser.version(), syntheticApis: true, scenarios: [] };
  try {
    for (const width of [390, 1440]) {
      let approved = false, approvals = 0, starts = 0, lifetime = 300;
      const requester = await fixturePage(browser, width, 'dark');
      const approver = await fixturePage(browser, 390, 'dark');
      let requesterAuthenticated = false;
      await requester.page.route('**/api/v1/auth/me', route => route.fulfill(requesterAuthenticated ? { json: identity } : { status: 401, json: {} }));
      await requester.page.route('**/api/v1/auth/bootstrap/status', route => route.fulfill({ json: { needsBootstrap: false, localLoginEnabled: false } }));
      await requester.page.route('**/api/v1/auth/oidc/status', route => route.fulfill({ json: { enabled: true, localLoginEnabled: false, provider: 'Synthetic SSO' } }));
      await requester.page.route('**/api/v1/auth/device/start', route => {
        starts += 1;
        return route.fulfill({ json: { secret: 'a'.repeat(64), code: 'ABCD1234', expiresIn: lifetime, qrWidth: qr.width, qrModules } });
      });
      await requester.page.route('**/api/v1/auth/device/poll', route => {
        assert.equal(route.request().postDataJSON().secret, 'a'.repeat(64));
        if (approved) requesterAuthenticated = true;
        return route.fulfill({ json: approved ? { status: 'authenticated', ...identity } : { status: 'pending' } });
      });
      await approver.page.route('**/api/v1/auth/me', route => route.fulfill({ json: identity }));
      await approver.page.route('**/api/v1/auth/device/inspect', route => {
        assert.equal(route.request().headers()['x-csrf-token'], identity.csrfToken);
        assert.equal(route.request().postDataJSON().code, 'ABCD1234');
        return route.fulfill({ json: { device: 'Chrome · Téléviseur synthétique' } });
      });
      await approver.page.route('**/api/v1/auth/device/approve', route => {
        assert.equal(route.request().headers()['x-csrf-token'], identity.csrfToken);
        approved = true; approvals += 1;
        return route.fulfill({ status: 204 });
      });
      await requester.page.goto(base + 'login/');
      await requester.page.getByRole('button', { name: 'Afficher le QR code', exact: true }).waitFor();
      assert.equal(await requester.page.locator('input[autocomplete="current-password"]').count(), 0);
      await requester.page.getByRole('button', { name: 'Afficher le QR code', exact: true }).click();
      await requester.page.getByRole('img', { name: /QR code de connexion/ }).waitFor();
      assert.equal(await requester.page.evaluate(() => localStorage.getItem('media.pending-device-code')), null);
      assert.equal(await requester.page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await requester.page.screenshot({ path: `${out}/login-${width}.png`, fullPage: true });
      await approver.page.goto(base + 'connect/#ABCD1234');
      await approver.page.getByLabel('Code du nouvel appareil').waitFor();
      assert.equal(await approver.page.getByLabel('Code du nouvel appareil').inputValue(), 'ABCD1234');
      assert.equal(approvals, 0, 'Scanning never automatically approves');
      await approver.page.getByRole('button', { name: 'Vérifier le code', exact: true }).click();
      await approver.page.getByText('Chrome · Téléviseur synthétique', { exact: true }).waitFor();
      await approver.page.getByRole('button', { name: 'Annuler', exact: true }).click();
      assert.equal(approvals, 0);
      await approver.page.getByRole('button', { name: 'Vérifier le code', exact: true }).click();
      await approver.page.getByRole('button', { name: 'Autoriser la connexion', exact: true }).waitFor();
      await approver.page.screenshot({ path: `${out}/confirmation-${width}.png`, fullPage: true });
      await approver.page.getByRole('button', { name: 'Autoriser la connexion', exact: true }).click();
      await approver.page.getByText('Connexion autorisée.', { exact: false }).waitFor();
      await requester.page.waitForURL(base);
      await requester.page.locator('.catalog-home').waitFor();
      assert.equal(approvals, 1);
      assert.deepEqual(requester.errors, []); assert.deepEqual(approver.errors, []);
      await requester.context.close(); await approver.context.close();

      // A phone without an active session resumes the scanned code after SSO.
      const resume = await fixturePage(browser, width, 'dark');
      let authenticated = false;
      await resume.page.route('**/api/v1/auth/me', route => route.fulfill(authenticated ? { json: identity } : { status: 401, json: {} }));
      await resume.page.route('**/api/v1/auth/bootstrap/status', route => route.fulfill({ json: { needsBootstrap: false, localLoginEnabled: false } }));
      await resume.page.route('**/api/v1/auth/oidc/status', route => route.fulfill({ json: { enabled: true, localLoginEnabled: false } }));
      await resume.page.route('**/api/v1/auth/oidc/start', route => { authenticated = true; return route.fulfill({ json: { url: base } }); });
      await resume.page.goto(base + 'connect/#ABCD1234');
      await resume.page.waitForURL(base + 'login/');
      assert.equal(await resume.page.evaluate(() => sessionStorage.getItem('media.pending-device-code')), 'ABCD1234');
      await resume.page.getByRole('button', { name: 'Se connecter avec SSO', exact: true }).click();
      await resume.page.waitForURL(base + 'connect/');
      await resume.page.getByLabel('Code du nouvel appareil').waitFor();
      assert.equal(await resume.page.getByLabel('Code du nouvel appareil').inputValue(), 'ABCD1234');
      assert.equal(await resume.page.evaluate(() => sessionStorage.getItem('media.pending-device-code')), null);
      assert.deepEqual(resume.errors, []);
      await resume.context.close();

      // A short synthetic lifetime exercises the actual expiry UI without waiting 5 minutes.
      const expiry = await fixturePage(browser, width, 'dark');
      await expiry.page.route('**/api/v1/auth/me', route => route.fulfill({ status: 401, json: {} }));
      await expiry.page.route('**/api/v1/auth/device/start', route => route.fulfill({ json: { secret: 'b'.repeat(64), code: 'ABCD5678', expiresIn: 1, qrWidth: 21, qrModules: Array(441).fill(true) } }));
      await expiry.page.goto(base + 'login/');
      await expiry.page.getByRole('button', { name: 'Afficher le QR code', exact: true }).click();
      await expiry.page.getByText('Ce QR code a expiré.', { exact: false }).waitFor();
      assert.equal(await expiry.page.getByRole('img', { name: /QR code de connexion/ }).count(), 0);
      await expiry.page.getByRole('button', { name: 'Générer un nouveau code', exact: true }).click();
      await expiry.page.getByRole('img', { name: /QR code de connexion/ }).waitFor();
      assert.deepEqual(expiry.errors, []);
      await expiry.context.close();
      report.scenarios.push({ width, ssoOnlyLogin: true, qrVisible: true, explicitConfirmation: true, cancelPreservesRequest: true, approvedPollConnects: true, ssoResume: true, expiryRenewal: true, starts });
    }
    report.passed = true;
  } catch (error) { report.failure = String(error.stack || error); throw error; }
  finally { await browser.close(); fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2)); console.log(JSON.stringify(report)); }
}
run().catch(error => { console.error(error); process.exitCode = 1; });

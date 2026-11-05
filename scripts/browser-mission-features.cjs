// Production Svelte UI with explicit synthetic APIs. No Core/provider/delivery proof.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-details.cjs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5183/app/';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/mission-features';

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const { page, context, errors } = await setup(browser, 1440, 'dark');
    let role = 'administrator';
    let recommendation = { enabled: false, excludeWatched: true, historyDays: 180 };
    let notification = { enabled: false, scans: false, tasks: false, content: false };
    let savedChannel;
    let mutationCount = 0;
    const csrf = () => role === 'administrator' ? 'synthetic-test' : 'synthetic-user';
    await page.route('**/api/v1/auth/me', route => route.fulfill({ json: { user: { id: role === 'administrator' ? 'visual' : 'user', login: 'synthetic', role }, csrfToken: csrf() } }));
    await page.route('**/api/v1/account/recommendations**', route => {
      const request = route.request();
      if (request.method() === 'PUT') {
        assert.equal(request.headers()['x-csrf-token'], csrf());
        recommendation = request.postDataJSON(); mutationCount++;
      }
      if (new URL(request.url()).pathname.endsWith('/preferences')) return route.fulfill({ json: recommendation });
      return route.fulfill({ json: { enabled: recommendation.enabled, mode: recommendation.enabled ? 'personalized' : 'disabled', items: recommendation.enabled ? [{ id: 'local-recommendation', kind: 'film', title: 'Découverte locale', year: 2026, synthetic: false, libraryId: 'film', posterUrl: '/ux-art/poster', recommendationReason: 'Selon votre intérêt pour Les horizons', genres: [], editions: [] }] : [], inspectedCount: 12, candidateLimit: 1000, truncated: false } });
    });
    await page.route('**/api/v1/account/notifications', route => {
      const request = route.request();
      if (request.method() === 'PUT') {
        assert.equal(request.headers()['x-csrf-token'], csrf());
        notification = request.postDataJSON(); mutationCount++;
        return route.fulfill({ status: 204 });
      }
      return route.fulfill({ json: notification });
    });
    await page.route('**/api/v1/admin/notifications/**', route => {
      const request = route.request();
      assert.equal(role, 'administrator', 'user UI must not request administrator configuration');
      if (request.method() === 'POST') {
        assert.equal(request.headers()['x-csrf-token'], csrf());
        const input = request.postDataJSON();
        assert.equal(input.enabled, false, 'test must keep external destination disabled');
        assert.equal(input.config.destination, 'https://receiver.example.org/events');
        assert.equal(input.kind, 'webhook');
        assert.ok(input.config.secret.length >= 32);
        const { secret: _secret, ...config } = input.config;
        savedChannel = { ...input, id: 'fixture-channel', config: { ...config, hasSecret: true } };
        mutationCount++;
        return route.fulfill({ json: { id: savedChannel.id } });
      }
      const pathname = new URL(request.url()).pathname;
      return route.fulfill({ json: pathname.endsWith('/history') ? { items: [] } : { items: savedChannel ? [savedChannel] : [], users: [{ id: 'visual', role: 'administrator' }], events: ['task.completed', 'task.failed', 'scan.completed'] } });
    });

    await page.goto(base + 'settings/#display');
    const recommendations = page.getByRole('region', { name: 'Recommandations personnelles' });
    await recommendations.getByRole('button', { name: 'Enregistrer les recommandations' }).waitFor();
    assert.equal(await recommendations.getByLabel('Activer les recommandations sur l’accueil').isChecked(), false);
    await recommendations.getByLabel('Activer les recommandations sur l’accueil').check();
    await recommendations.getByLabel('Historique utilisé (jours)').fill('30');
    await recommendations.getByRole('button', { name: 'Enregistrer les recommandations' }).click();
    await recommendations.getByRole('status').getByText('Préférences enregistrées.').waitFor();
    assert.deepEqual(recommendation, { enabled: true, excludeWatched: true, historyDays: 30 });
    await page.goto(base);
    await page.getByRole('heading', { name: 'Recommandé pour vous', exact: true }).waitFor();
    await page.getByText('Selon votre intérêt pour Les horizons', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(out, 'recommendations.png') });
    await page.goto(base + 'settings/#display');
    await recommendations.getByLabel('Activer les recommandations sur l’accueil').uncheck();
    await recommendations.getByRole('button', { name: 'Enregistrer les recommandations' }).click();
    await recommendations.getByRole('status').getByText('Préférences enregistrées.').waitFor();
    await page.goto(base);
    await page.getByRole('heading', { name: 'Films récemment ajoutés', exact: true }).waitFor();
    assert.equal(await page.getByRole('heading', { name: 'Recommandé pour vous', exact: true }).count(), 0);

    await page.goto(base + 'settings/#security');
    const notifications = page.getByRole('region', { name: 'Notifications', exact: true });
    await notifications.getByLabel('Recevoir les notifications sur mes destinations configurées').check();
    await notifications.getByLabel('Scans terminés de mes bibliothèques autorisées').check();
    await notifications.getByRole('button', { name: 'Enregistrer mes préférences' }).click();
    await notifications.getByRole('status').getByText('Préférences enregistrées.').waitFor();
    await notifications.getByText('Destinations et historique administrateur', { exact: true }).click();
    await notifications.getByLabel('Nom', { exact: true }).fill('Récepteur de test désactivé');
    await notifications.getByLabel('URL HTTPS du récepteur', { exact: true }).fill('https://receiver.example.org/events');
    await notifications.getByLabel('Secret HMAC partagé (32 caractères minimum)', { exact: true }).fill('synthetic-webhook-secret-for-offline-test');
    await notifications.getByRole('button', { name: 'Enregistrer la destination' }).click();
    await notifications.getByText('Récepteur de test désactivé · webhook · Désactivée', { exact: true }).waitFor();
    await notifications.getByRole('button', { name: 'Modifier', exact: true }).click();
    assert.equal(await notifications.getByLabel('Secret HMAC partagé (32 caractères minimum)', { exact: true }).inputValue(), '');
    assert.equal(await notifications.getByLabel('Activer les futurs envois vers cette destination').isChecked(), false);
    await notifications.getByText('Aucune notification enregistrée.', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(out, 'notifications-admin.png') });

    role = 'user'; notification = { enabled: false, scans: false, tasks: false, content: false };
    await page.reload();
    await notifications.getByLabel('Recevoir les notifications sur mes destinations configurées').check();
    assert.equal(await notifications.getByLabel('Nouveaux films, séries et épisodes autorisés après activation').isChecked(), false);
    await notifications.getByLabel('Nouveaux films, séries et épisodes autorisés après activation').check();
    await notifications.getByRole('button', { name: 'Enregistrer mes préférences' }).click();
    await notifications.getByRole('status').getByText('Préférences enregistrées.').waitFor();
    assert.equal(notification.tasks, false);
    assert.equal(notification.content, true);
    assert.equal(await notifications.getByText('Destinations et historique administrateur', { exact: true }).count(), 0);
    await page.screenshot({ path: path.join(out, 'notifications-user.png') });
    assert.deepEqual(errors, []);
    const report = { passed: true, browser: browser.version(), scope: `${process.env.MEDIA_UI_MODE || 'Production Svelte build'} and intercepted synthetic APIs only; no Core, provider, SMTP or external delivery proof.`, mutationCount, checks: ['recommendations opt-in, history days, CSRF, explanation and opt-out', 'notification account preferences, content opt-in for user, administrator-only destination UI, disabled webhook, masked stored secret and empty history'], screenshots: ['recommendations.png', 'notifications-admin.png', 'notifications-user.png'] };
    fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
    await context.close();
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });

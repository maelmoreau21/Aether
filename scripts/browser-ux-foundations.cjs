// Real production Svelte UI; all API data and artwork below are explicit synthetic fixtures.
// Run after npm run build and npx vite preview --host 127.0.0.1 --port 5182.
const { chromium } = require('../apps/web/node_modules/playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/ux-foundations';
fs.mkdirSync(out, { recursive: true });
const screens = [], checks = [], auditIssues = [];
const films = Array.from({ length: 20 }, (_, i) => ({ id: `film-${i}`, kind: 'film', title: i === 1 ? 'Un voyage à travers les paysages et les histoires de notre mémoire' : `Les horizons — ${i + 1}`, year: 2026, synthetic: true, description: 'Une exploration des paysages, de la lumière et des liens qui nous unissent. Synopsis synthétique réservé aux tests de composition.', posterUrl: `/ux-art/poster-${i}`, artwork: { backdrop: '/ux-art/backdrop' }, libraryId: 'film', editions: [] }));
const series = films.slice(0, 12).map(work => ({ ...work, id: work.id + 's', kind: 'series', title: `Les saisons — ${work.id.slice(5)}`, libraryId: 'series', seasonCount: 2, episodeCount: 4, seasons: [1, 2].map(number => ({ id: `season-${number}`, number, title: `Saison ${number}`, episodes: [1, 2].map(ep => ({ ...films[0], id: `ep-${number}-${ep}`, kind: 'episode', title: `Épisode ${ep}`, number: ep, artwork: { thumb: '/ux-art/backdrop' } })) })) }));
const libraries = [{ id: 'film', name: 'Films', kind: 'films' }, { id: 'series', name: 'Séries', kind: 'series' }, { id: 'music', name: 'Musique', kind: 'music' }, ...Array.from({ length: 18 }, (_, i) => ({ id: `custom-${i}`, name: `Bibliothèque personnelle ${i + 1} — Archives et souvenirs de voyage`, kind: 'films' }))];
async function fixturePage(browser, width, theme, mode = 'normal', height = 900) {
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, hasTouch: width < 901, serviceWorkers: 'block', reducedMotion: 'reduce' });
  const page = await context.newPage();
  const errors = [];
  let libraryFailure = mode === 'library-error';
  page.on('pageerror', error => errors.push(String(error)));
  await page.route('**/ux-art/**', route => {
    if (mode === 'broken') return route.fulfill({ status: 404, body: '' });
    if (route.request().url().endsWith('logo')) return route.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="140"><text x="0" y="90" fill="#b38a49" font-size="54" font-family="serif">LES HORIZONS</text></svg>' });
    const back = route.request().url().endsWith('backdrop');
    const width = back ? 1280 : 400;
    return route.fulfill({ contentType: 'image/svg+xml', body: `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${back ? 720 : 600}" viewBox="0 0 ${width} ${back ? 720 : 600}"><rect width="100%" height="100%" fill="#263d42"/><circle cx="${back ? 970 : 240}" cy="210" r="145" fill="#c8ae7c"/><path d="M0 720L260 380L520 720L920 310L1280 720" fill="#142b31"/><path d="M0 720L450 570L800 720L1120 480L1280 720" fill="#355d62"/></svg>` });
  });
  await page.route('**/api/v1/**', async route => {
    const u = new URL(route.request().url()), p = u.pathname;
    let data = { sessions: [], extensions: [], collections: [], packs: [], images: [], groups: [], users: [], libraries: [], nodes: [], roots: [], localRoots: [], scans: [], items: [], lists: [] };
    if (p.endsWith('/auth/me')) data = { user: { id: 'visual', login: 'visual', role: 'administrator' }, csrfToken: 'synthetic-test' };
    else if (p === '/api/v1/preferences') data = { locale: 'fr', theme };
    else if (p === '/api/v1/server') data = { serverName: 'Aether', welcomeMessage: '' };
    else if (p === '/api/v1/config') data = { name: 'Aether', mode: 'standalone', version: 'visual-test' };
    else if (p === '/api/v1/libraries') {
      if (libraryFailure) { libraryFailure = false; return route.fulfill({ status: 503, json: {} }); }
      data = { libraries: mode === 'empty' ? [] : libraries };
    }
    else if (p === '/api/v1/catalog') {
      if (mode === 'error') return route.fulfill({ status: 503, json: {} });
      if (mode === 'loading') await new Promise(resolve => setTimeout(resolve, 1500));
      const items = mode === 'empty' ? [] : u.searchParams.get('kind') === 'series' ? series : films;
      const displayed = items.map(work => mode === 'no-backdrop' ? { ...work, artwork: {} } : (mode === 'logo' || mode === 'broken') ? { ...work, artwork: { ...work.artwork, logo: '/ux-art/logo' } } : mode === 'unavailable' ? { ...work, available: false } : work);
      data = { items: displayed, total: items.length, offset: 0, limit: 24, hasMore: false, notice: '' };
    }
    else if (p.startsWith('/api/v1/catalog/')) data = p.endsWith('film-0s') ? series[0] : films[0];
    else if (p === '/api/v1/progress') data = { items: mode === 'empty' ? [] : Array.from({ length: 5 }, (_, i) => ({ workId: `film-${i}`, title: films[i].title, kind: 'film', position: 120, duration: 600, watched: false, revision: 1 })) };
    else if (p.includes('/progress/')) data = { position: 120, duration: 600, watched: false, revision: 1 };
    else if (p === '/api/v1/music') data = { items: [{ id: 'track', title: 'Étude sonore', artist: 'Atelier de test', albumArtist: 'Atelier de test', album: 'Lumière', duration: 125, codec: 'opus', mime: 'audio/ogg', trackNumber: 1 }], total: 1 };
    else if (p === '/api/v1/account') data = { user: { id: 'visual', login: 'visual', role: 'administrator' }, avatar: null };
    else if (p === '/api/v1/health') data = { status: 'ok', mode: 'standalone', version: 'visual-test' };
    else if (p === '/api/v1/account/avatar/policy') data = { uploadAllowed: false, removeAllowed: true };
    return route.fulfill({ json: data });
  });
  return { page, context, errors };
}
async function capture(page, name, fullPage = true) {
  await page.screenshot({ path: `${out}/${name}.png`, fullPage });
  screens.push(name);
}
async function noOverflow(page, label) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${label}: document overflow`);
  // Check actual visible header children too: body overflow:hidden can hide defects.
  const clipped = await page.locator('.header-inner').evaluateAll(headers => headers.flatMap(header => [...header.children].filter(child => { const r = child.getBoundingClientRect(); return r.left < 0 || r.right > innerWidth; }).map(el => el.className)));
  assert.deepEqual(clipped, [], `${label}: clipped header`);
}
async function run() {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    for (const theme of ['light', 'dark']) for (const width of [320, 360, 390, 430, 768, 1024, 1440, 1920]) {
      const { page, context, errors } = await fixturePage(browser, width, theme);
      await page.goto(base);
      await page.locator('.catalog-home').waitFor();
      await page.locator('.continue-card').first().waitFor();
      await page.locator('.hero-banner .backdrop-img').evaluate(img => img.decode());
      await noOverflow(page, `${theme}-${width}`);
      assert.equal(await page.locator('.hero-actions > *').count(), 2);
      assert.equal(await page.locator('.catalog-home > .shelf-container h2').first().textContent(), 'Films récemment ajoutés');
      assert.equal(await page.locator('.catalog-home > .shelf-container').count(), 2);
      const poster = await page.locator('.poster').first().boundingBox();
      assert.ok(Math.abs(poster.width / poster.height - 2 / 3) < .01);
      await capture(page, `${theme}-${width}-home`);
      if (width === 390 || width === 1440) {
        await capture(page, [theme,width,'home-viewport'].join('-'), false);
        const row = page.locator('.shelf-track').first();
        await row.locator('a').nth(8).focus();
        assert.ok(await row.evaluate(el => el.scrollLeft > 0), 'Keyboard focus must reveal offscreen cards');
        await page.locator('.hero-actions a').click();
        await page.locator('.film-detail-view').waitFor();
        await page.goto(base); await page.locator('.catalog-home').waitFor();
        await page.locator('.hero-actions button').click();
        await page.locator('.film-detail-view').waitFor();
        await page.goto(base); await page.locator('.catalog-home').waitFor();
      }
      const trigger = page.locator(width <= 900 ? '.mobile-nav button' : '.desktop-nav button');
      await trigger.focus();
      await page.keyboard.press('Enter');
      await page.getByRole('dialog', { name: 'Bibliothèques', exact: true }).waitFor();
      assert.equal(await page.locator('.library-list a').count(), libraries.length);
      const overlap = await page.locator('.library-list').evaluate(list => {
        const rows = [...list.children];
        return rows.some((row, i) => i > 0 && row.getBoundingClientRect().top < rows[i - 1].getBoundingClientRect().bottom);
      });
      assert.equal(overlap, false, 'Long library names must not overlap adjacent rows');
      const box = await trigger.boundingBox();
      assert.ok(box.width >= 44 && box.height >= 44);
      await page.locator('.downloads-link').focus();
      await page.keyboard.press('Tab');
      assert.equal(await page.evaluate(() => document.activeElement === document.body || document.activeElement.closest('dialog')?.id === 'library-navigation'), true, 'Tab must not enter the inert background');
      await page.getByRole('button', { name: 'Fermer les bibliothèques' }).focus();
      await capture(page, `${theme}-${width}-libraries`, false);
      await page.keyboard.press('Escape');
      await page.waitForFunction(el => el.getAttribute('aria-expanded') === 'false', await trigger.elementHandle());
      assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
      assert.equal(await trigger.evaluate(el => el === document.activeElement), true);
      assert.equal(await trigger.evaluate(el => getComputedStyle(el).outlineStyle), 'solid');
      await trigger.click();
      await page.getByRole('link', { name: 'Films Films', exact: true }).click();
      await page.locator('.catalog-grid-view').waitFor();
      assert.equal(new URL(page.url()).searchParams.get('library'), 'film');
      await noOverflow(page, `${theme}-${width}-catalog`);
      await capture(page, `${theme}-${width}-catalog`);
      if (width === 390 || width === 1440) {
        for (const [path, selector, name] of [['films/?work=film-0', '.film-detail-view', 'film-detail'], ['series/?work=film-0s', '.series-detail-view', 'series-detail'], ['search/', '.catalog-grid-view', 'search'], ['music/', '.tracks', 'music'], ['lists/', '.workspace', 'lists'], ['settings/', '.settings-view', 'settings'], ['admin/libraries/', '.admin-dashboard-container', 'admin'], ['downloads/', 'main', 'downloads']]) {
          await page.goto(base + path);
          try { await page.locator(selector).waitFor({ timeout: 5000 }); await noOverflow(page, name); }
          catch (error) { auditIssues.push({ theme, width, path, error: String(error), pageErrors: [...errors] }); }
          await capture(page, `${theme}-${width}-${name}`);
        }
        await page.goto(base + 'music/');
        await page.getByRole('button', { name: 'Lire Étude sonore', exact: true }).click();
        await page.locator('.music-player').waitFor();
        if (width < 901) {
          const nav = await page.locator('.mobile-nav').boundingBox(), player = await page.locator('.music-player').boundingBox();
          assert.ok(player.y + player.height <= nav.y + 1, 'Music player must remain above bottom navigation');
        }
        await capture(page, `${theme}-${width}-music-player`, false);
        await page.goto(base);
        await page.locator('.catalog-home').waitFor();
        await page.getByRole('button', { name: 'Menu profil', exact: true }).click();
        await page.locator('#profile-menu:popover-open').waitFor();
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#profile-menu:popover-open').count(), 0);
      }
      assert.deepEqual(errors, [], `${theme}-${width}: runtime errors`);
      checks.push({ theme, width, home: true, menuKeyboard: true, manyLibraries: true, libraryNavigation: true, overflow: false });
      await context.close();
      console.log(`PASS ${theme} ${width}`);
    }
    for (const mode of ['broken', 'empty', 'error', 'library-error', 'loading', 'logo', 'no-backdrop', 'unavailable']) {
      const { page, context, errors } = await fixturePage(browser, 320, 'dark', mode);
      await page.goto(base);
      if (mode === 'loading') { await page.getByRole('status').filter({ hasText: 'Chargement' }).first().waitFor(); await capture(page, `state-${mode}`, false); }
      if (mode === 'error') await page.getByRole('heading', { name: 'Le catalogue ne répond pas' }).waitFor();
      else { await page.locator('.catalog-home').waitFor(); if (mode === 'empty') assert.equal(await page.locator('.continue-shelf').count(), 0); }
      if (mode === 'broken') { await page.locator('.hero-title-logo').count(); await page.waitForTimeout(200); assert.equal(await page.locator('.poster-fallback').count() > 0, true); }
      if (mode === 'library-error') { await page.locator('.mobile-nav button').click(); await page.getByRole('button', { name: 'Réessayer', exact: true }).waitFor(); await capture(page, 'state-library-error-before-retry', false); await page.getByRole('button', { name: 'Réessayer', exact: true }).click(); await page.waitForFunction(() => document.querySelectorAll('.library-list a').length === 21); }
      if (mode === 'logo') { assert.equal(await page.locator('.hero-title-logo').count(), 1); assert.equal(await page.locator('.hero-banner h1').innerText(), ''); }
      if (mode === 'no-backdrop') assert.equal(await page.locator('.backdrop-img').count(), 0);
      if (mode === 'unavailable') assert.equal(await page.locator('.hero-actions button').isDisabled(), true);
      await noOverflow(page, mode);
      await capture(page, `state-${mode}`);
      assert.deepEqual(errors, []);
      checks.push({ mode, width: 320, pass: true });
      await context.close();
    }
    const { page, context } = await fixturePage(browser, 844, 'dark', 'normal', 390);
    await page.goto(base); await page.locator('.catalog-home').waitFor(); await noOverflow(page, 'landscape'); await capture(page, 'dark-landscape', false);
    await page.locator('.mobile-nav button').tap(); await capture(page, 'dark-landscape-libraries', false);
    assert.ok(await page.locator('.library-dialog').evaluate(el => el.clientHeight <= innerHeight));
    await page.getByRole('button', { name: 'Fermer les bibliothèques' }).tap();
    await context.close(); checks.push({ landscapeTouch: true });
  } finally {
    fs.writeFileSync(`${out}/report.json`, JSON.stringify({ browser: browser.version(), scope: 'Production Svelte UI; synthetic intercepted APIs/artwork; no real server, NAS or physical device claim', checks, screens, auditIssues }, null, 2));
    await browser.close();
  }
  assert.deepEqual(auditIssues, [], 'Audited routes must render without regression');
  console.log(`Completed ${checks.length} scenarios and ${screens.length} screenshots; audit issues: ${auditIssues.length}`);
}
if (require.main === module) run().catch(error => { console.error(error); process.exitCode = 1; });
module.exports = { fixturePage };




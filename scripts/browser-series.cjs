const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const out = 'docs/evidence/2026-10-17/series';
fs.mkdirSync(out, {recursive:true});

(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  const report = {browser:browser.version(), os:require('node:os').release(), headless:true, results:{}};
  try {
    const context = await browser.newContext({viewport:{width:1440,height:900}});
    const page = await context.newPage();
    const errors = [];
    const requestedMediaUrls = [];

    page.on('response', async response => {
      if (response.url().includes('/api/v1/media/')) {
        requestedMediaUrls.push(response.url());
      }
      if (response.url().endsWith('/scan')) {
        console.log('scan HTTP', response.status(), await response.text());
      }
    });
    page.on('pageerror', error => errors.push(String(error)));
    page.on('console', message => {
      if (message.type() === 'error' && /Content Security Policy/i.test(message.text())) {
        errors.push(message.text());
      }
    });

    // 1. Login
    await page.goto(origin + '/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('series-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable series browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin + '/app/');

    // 2. Admin libraries: add Séries library and scan
    await page.goto(origin + '/app/admin/libraries/');
    await page.locator('#library-name').fill('Séries Navigateur');
    await page.locator('#library-kind').selectOption('series');
    await page.getByRole('button', {name:'Enregistrer', exact:true}).click();
    await page.getByRole('heading', {name:'Séries Navigateur', exact:true}).waitFor();

    await page.getByRole('button', {name:'Scanner Séries Navigateur', exact:true}).click();
    const article = page.locator('article').filter({has: page.getByRole('heading', {name:'Séries Navigateur', exact:true})});
    await article.getByText(/Scan : Terminé/).waitFor({timeout:60000});
    report.results.scan = true;
    await page.screenshot({path:`${out}/admin-scan.png`});

    // 3. Open Series catalogue
    await page.goto(origin + '/app/series/');
    const card = page.locator('.media-card').filter({hasText:'Browser Series'});
    await card.waitFor();
    assert.equal(await page.locator('.media-card').count(), 1);

    // Record media URLs requested before clicking card
    const mediaCallsBeforeDetail = requestedMediaUrls.length;

    // 4. Click Series Card -> open Detail
    await card.click();
    await page.getByRole('heading', {name:'Browser Series', exact:true}).waitFor();

    // Verify: Opening series detail did NOT trigger batch /api/v1/media/ requests!
    const mediaCallsAfterDetail = requestedMediaUrls.length;
    assert.equal(
      mediaCallsAfterDetail,
      mediaCallsBeforeDetail,
      'Opening series detail must not pre-fetch episode media descriptors'
    );
    report.results.deferredMediaLoading = true;

    // Verify seasons dropdown and episode list
    const seasonSelect = page.locator('select[aria-label="Saison"]');
    await seasonSelect.waitFor();
    await page.screenshot({path:`${out}/series-detail.png`});

    // 5. Select Version and Play Episode 1
    const ep1PlayButton = page.getByRole('button', {name:'Lire Episode One', exact:true});
    await ep1PlayButton.waitFor();
    await ep1PlayButton.click();

    // Verify /api/v1/media/ was requested ONLY upon launch
    await page.waitForFunction(() => document.querySelector('video') !== null);
    await page.waitForFunction(() => document.querySelector('video')?.currentTime > 0.1);
    report.results.episode1Playback = true;

    // Pause and seek test
    await page.evaluate(() => {
      const v = document.querySelector('video');
      v.pause();
      v.currentTime = 2.0;
    });
    await page.waitForFunction(() => {
      const v = document.querySelector('video');
      return v.paused && Math.abs(v.currentTime - 2.0) < 0.5;
    });
    report.results.seekAndPause = true;
    await page.screenshot({path:`${out}/immersive-playback.png`});

    // Close immersive player
    const closeBtn = page.getByRole('button', {name: 'Retour à l’accueil'});
    await closeBtn.click({force: true});
    await page.waitForFunction(() => !document.querySelector('dialog#player')?.hasAttribute('open'));

    // 6. Navigate to Season 2
    await seasonSelect.selectOption('2');
    const ep2PlayButton = page.getByRole('button', {name:'Lire Season Two Episode', exact:true});
    await ep2PlayButton.waitFor();
    report.results.seasonNavigation = true;

    // Launch Episode in Season 2
    await ep2PlayButton.click();
    await page.waitForFunction(() => document.querySelector('video') !== null);
    await page.waitForFunction(() => document.querySelector('video')?.currentTime > 0.1);
    report.results.season2Playback = true;

    // Close immersive player after Season 2 playback (S02E01 is now in descriptorCache)
    await closeBtn.click({force: true});
    await page.waitForFunction(() => !document.querySelector('dialog#player')?.hasAttribute('open'));

    // 7. Revoke permissions (simulate HTTP 403 on /api/v1/media/) -> must revalidate on launch, evict cache, and block playback
    const mediaCallsBefore403 = requestedMediaUrls.length;
    await page.route('**/api/v1/media/**', route => route.fulfill({status: 403}));
    await ep2PlayButton.click();
    const alert403 = page.getByRole('alert').filter({hasText: /n’a pas accès à cette ressource/});
    await alert403.waitFor();
    assert(
      requestedMediaUrls.length > mediaCallsBefore403,
      'Launching playback must revalidate permissions with the server even when descriptor was cached'
    );
    assert.equal(
      await page. evaluate(() => Boolean(document.querySelector('dialog#player')?.hasAttribute('open'))),
      false,
      'Player must not open when revalidation returns HTTP 403'
    );
    report.results.revokedPermission403 = true;

    // 8. Restore permissions -> playback must succeed again
    await page.unroute('**/api/v1/media/**');
    const mediaCallsBeforeRestore = requestedMediaUrls.length;
    await ep2PlayButton.click();
    await page.waitForFunction(() => Boolean(document.querySelector('dialog#player')?.hasAttribute('open')));
    await page.waitForFunction(() => document.querySelector('video')?.currentTime > 0.1);
    assert(
      requestedMediaUrls.length > mediaCallsBeforeRestore,
      'Restored playback must re-fetch descriptor from server'
    );
    report.results.restoredPermissionPlayback = true;

    await closeBtn.click({force: true});
    await page.waitForFunction(() => !document.querySelector('dialog#player')?.hasAttribute('open'));

    // 9. Account switch in SPA (User A -> cache filled -> logout -> login User B without page reload)
    await seasonSelect.selectOption('1');
    const ep1Select = page.locator('select[aria-label="Version de Episode One"]');
    await ep1Select.waitFor();

    // Focus Episode 1 version selector in Session A to ensure all its versions are prefetched and cached
    await ep1Select.focus();
    await page.waitForFunction(
      () => {
        const sel = document.querySelector('select[aria-label="Version de Episode One"]');
        return sel && Array.from(sel.options).every(opt => opt.textContent === 'MP4' || opt.textContent === 'MKV');
      },
      null,
      {timeout: 5000}
    );
    const mediaCallsAfterPrefetchA = requestedMediaUrls.length;

    // Re-focus in Session A: must hit descriptorCache with 0 new network calls
    await ep1Select.blur();
    await ep1Select.focus();
    await page.waitForTimeout(100);
    assert.equal(
      requestedMediaUrls.length,
      mediaCallsAfterPrefetchA,
      'Same-session prefetch must use descriptorCache without extra network calls'
    );

    // Logout User A via ProfileMenu (pure SPA transition, no page.goto/reload)
    await page.getByRole('button', {name: 'Menu profil', exact: true}).click();
    await page.getByRole('button', {name: 'Se déconnecter', exact: true}).click();
    await page.waitForURL('**/app/login/**');

    // Intercept auth responses to represent User B while preserving Core session cookie & CSRF token
    await page.route('**/api/v1/auth/**', async route => {
      const url = route.request().url();
      if (url.endsWith('/login') || url.endsWith('/me')) {
        const upstream = await route.fetch();
        if (upstream.status() === 200) {
          const body = await upstream.json();
          body.user = {id: 'user-b-browser', login: 'series-user-b', role: 'administrator'};
          await route.fulfill({response: upstream, json: body});
          return;
        }
        await route.fulfill({response: upstream});
        return;
      }
      await route.continue();
    });

    // Login as User B via SPA login form
    await page.locator('input[autocomplete="username"]').fill('series-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable series browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin + '/app/');

    // Verify ProfileMenu shows User B
    assert.match(await page.locator('.profile-trigger').textContent(), /series-user-b/);

    // Navigate via SPA links to Series -> Browser Series (no full page reload)
    await page.getByRole('navigation', {name: 'Navigation principale'}).getByRole('link', {name: 'Séries', exact: true}).click();
    await card.waitFor();
    const mediaCallsBeforeDetailB = requestedMediaUrls.length;
    await card.click();
    await page.getByRole('heading', {name: 'Browser Series', exact: true}).waitFor();

    // Verify opening series detail in Session B still makes 0 eager media calls
    assert.equal(
      requestedMediaUrls.length,
      mediaCallsBeforeDetailB,
      'Opening series detail in Session B must not pre-fetch episode descriptors'
    );

    // Verify Episode 1 options reverted to uncached generic labels ('Version 1', 'Version 2') because Session A cache was purged!
    const ep1SelectB = page.locator('select[aria-label="Version de Episode One"]');
    await ep1SelectB.waitFor();
    const initialLabelsB = await ep1SelectB.locator('option').allTextContents();
    assert.deepEqual(
      initialLabelsB,
      ['Version 1', 'Version 2'],
      'Session A cached version labels (MP4/MKV) must not leak into Session B'
    );

    // Focus Episode 1 selector in Session B -> must fetch fresh descriptors from network
    const mediaCallsBeforePrefetchB = requestedMediaUrls.length;
    await ep1SelectB.focus();
    await page.waitForFunction(
      () => {
        const sel = document.querySelector('select[aria-label="Version de Episode One"]');
        return sel && Array.from(sel.options).every(opt => opt.textContent === 'MP4' || opt.textContent === 'MKV');
      },
      null,
      {timeout: 5000}
    );
    assert.equal(
      requestedMediaUrls.length - mediaCallsBeforePrefetchB,
      2,
      'Session B must perform fresh network requests for both versions after Session A cache purge'
    );
    report.results.accountSwitchCacheIsolation = true;

    assert.deepEqual(errors, []);
    report.results.cspErrors = errors;
    report.passed = true;
    await context.close();
  } catch (error) {
    report.failure = String(error.stack);
    process.exitCode = 1;
  } finally {
    await browser.close();
    fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  }
})();

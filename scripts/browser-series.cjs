const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const out = process.env.MEDIA_REPORT_DIR || 'docs/evidence/2026-10-17/series';
fs.rmSync(out, {recursive: true, force: true});
fs.mkdirSync(out, {recursive: true});

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

    // 1. Login as series-admin
    await page.goto(origin + '/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('series-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable series browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin + '/app/');

    // 2. Admin libraries: add Séries library, grant access to series-user-b, and scan
    await page.goto(origin + '/app/admin/libraries/');
    await page.locator('#library-name').fill('Séries Navigateur');
    await page.locator('#library-kind').selectOption('series');
    await page.getByLabel('series-user-b', {exact: true}).check();
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

    // P4: explicit next episode and user-controlled continuous playback across seasons.
    const nextButton=page.getByRole('button',{name:/^Épisode suivant :/});
    const nextTitle=(await nextButton.textContent()).replace('Épisode suivant : ','');
    await page.setViewportSize({width:390,height:844});
    const fits=await nextButton.evaluate(button=>{const r=button.getBoundingClientRect();return r.left>=0 && r.right<=innerWidth;});
    assert.ok(fits,'Next episode controls must fit on mobile');
    await page.screenshot({path:`${out}/next-episode-mobile.png`});
    await page.setViewportSize({width:1440,height:900});
    assert.equal(await page.getByLabel('Enchaîner automatiquement').isChecked(),false);
    const initialFullscreen=await page.evaluate(()=>Boolean(document.fullscreenElement));
    await nextButton.click();
    await page.waitForFunction(title=>document.querySelector('.top-bar .title>span')?.textContent===title,nextTitle);
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>.1);
    assert.equal(await page.evaluate(()=>Boolean(document.fullscreenElement)),initialFullscreen,'Episode transitions preserve fullscreen');
    await page.getByLabel('Enchaîner automatiquement').check();
    await page.evaluate(async()=>{const v=document.querySelector('video');v.currentTime=v.duration-.3;await v.play();});
    await page.waitForFunction(()=>document.querySelector('.top-bar .title>span')?.textContent==='Season Two Episode');
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>.1);
    report.results.nextEpisode={manual:true,automaticOptIn:true,crossSeason:true};

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
      await page.evaluate(() => Boolean(document.querySelector('dialog#player')?.hasAttribute('open'))),
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

    // 9a. Real account switch in SPA (series-admin -> cache filled -> logout -> login series-user-b with real HttpOnly cookies)
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

    const cookieAdmin = (await context.cookies()).find(c => c.name === 'media_session');
    assert(cookieAdmin && cookieAdmin.httpOnly && cookieAdmin.sameSite === 'Strict' && !cookieAdmin.secure);

    // Logout series-admin via ProfileMenu (pure SPA transition, real Core logout)
    await page.getByRole('button', {name: 'Menu profil', exact: true}).click();
    await page.getByRole('button', {name: 'Se déconnecter', exact: true}).click();
    await page.waitForURL('**/app/login/**');

    const cookieAfterLogout = (await context.cookies()).find(c => c.name === 'media_session');
    assert(!cookieAfterLogout, 'Logout Set-Cookie must remove HttpOnly media_session cookie');

    // Login as second distinct SQLite account series-user-b via SPA login form (no mocked auth JSON)
    await page.locator('input[autocomplete="username"]').fill('series-user-b');
    await page.locator('input[autocomplete="current-password"]').fill('disposable series user-b password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin + '/app/');

    const cookieUserB = (await context.cookies()).find(c => c.name === 'media_session');
    assert(cookieUserB && cookieUserB.httpOnly && cookieUserB.sameSite === 'Strict' && !cookieUserB.secure);
    assert.notEqual(cookieUserB.value, cookieAdmin.value, 'Second account must receive a distinct HttpOnly session cookie');

    // Verify ProfileMenu shows series-user-b
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

    // Focus Episode 1 selector in Session B -> must fetch fresh descriptors from network using series-user-b cookie
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

    // 9b. Delayed logout of A (series-user-b) + concurrent login of B (series-admin) with real HttpOnly Set-Cookie headers
    const logoutLoginSequence = [];
    await page.route('**/api/v1/auth/**', async route => {
      const url = route.request().url();
      if (url.endsWith('/logout')) {
        logoutLoginSequence.push('logout-request');
        await new Promise(r => setTimeout(r, 250));
        const upstream = await route.fetch();
        logoutLoginSequence.push('logout-response');
        await route.fulfill({response: upstream});
        return;
      }
      if (url.endsWith('/login')) {
        logoutLoginSequence.push('login-request');
        const upstream = await route.fetch();
        logoutLoginSequence.push('login-response');
        await route.fulfill({response: upstream});
        return;
      }
      await route.continue();
    });

    const delayedLogoutResult = await page.evaluate(async () => {
      const chunkUrls = performance
        .getEntriesByType('resource')
        .map(r => r.name)
        .filter(u => u.includes('/_app/immutable/chunks/'));
      let loginFn, logoutFn, sessionStore;
      for (const url of chunkUrls) {
        const mod = await import(url);
        for (const val of Object.values(mod)) {
          if (typeof val === 'function' && val.toString().includes('/api/v1/auth/login')) loginFn = val;
          if (typeof val === 'function' && val.toString().includes('/api/v1/auth/logout')) logoutFn = val;
          if (val && typeof val === 'object' && typeof val.subscribe === 'function' && typeof val.set === 'function') sessionStore = val;
        }
        if (loginFn && logoutFn && sessionStore) break;
      }
      if (!loginFn || !logoutFn || !sessionStore) throw new Error('Auth module exports not found');

      let currentVal = null;
      const unsub = sessionStore.subscribe(v => { currentVal = v; });
      const initialSession = currentVal;

      // Start logout of series-user-b (delayed 250ms on network)
      const pLogout = logoutFn(initialSession);
      // 20ms later, while logout is still in flight, start login of series-admin
      await new Promise(r => setTimeout(r, 20));
      const pLogin = loginFn('series-admin', 'disposable series browser password');
      await Promise.all([pLogout, pLogin]);
      unsub();

      const meRes = await fetch('/api/v1/auth/me', {credentials: 'same-origin', cache: 'no-store'});
      const meBody = meRes.ok ? await meRes.json() : null;
      return {
        storeSession: currentVal,
        meStatus: meRes.status,
        meBody
      };
    });
    await page.unroute('**/api/v1/auth/**');

    assert.deepEqual(
      logoutLoginSequence,
      ['logout-request', 'logout-response', 'login-request', 'login-response'],
      'Login HTTP request must be sequenced strictly after delayed logout HTTP response so Set-Cookie of logout never wipes the new session cookie'
    );
    assert.equal(delayedLogoutResult.meStatus, 200);
    assert.equal(delayedLogoutResult.meBody.user.login, 'series-admin');
    assert.equal(delayedLogoutResult.storeSession.user.login, 'series-admin');
    assert.equal(delayedLogoutResult.storeSession.csrfToken, delayedLogoutResult.meBody.csrfToken);
    const cookieAfterDelayedLogout = (await context.cookies()).find(c => c.name === 'media_session');
    assert(cookieAfterDelayedLogout && cookieAfterDelayedLogout.httpOnly);
    assert.notEqual(cookieAfterDelayedLogout.value, cookieUserB.value);
    report.results.delayedLogoutDoesNotWipeNewLoginCookie = true;

    // 9c. Two concurrent logins (series-admin delayed 250ms, series-user-b fast 0ms) with real HttpOnly Set-Cookie headers
    // First log out cleanly
    await page.evaluate(async () => {
      const chunkUrls = performance
        .getEntriesByType('resource')
        .map(r => r.name)
        .filter(u => u.includes('/_app/immutable/chunks/'));
      let logoutFn, sessionStore;
      for (const url of chunkUrls) {
        const mod = await import(url);
        for (const val of Object.values(mod)) {
          if (typeof val === 'function' && val.toString().includes('/api/v1/auth/logout')) logoutFn = val;
          if (val && typeof val === 'object' && typeof val.subscribe === 'function' && typeof val.set === 'function') sessionStore = val;
        }
        if (logoutFn && sessionStore) break;
      }
      let currentVal = null;
      const unsub = sessionStore.subscribe(v => { currentVal = v; });
      unsub();
      if (currentVal) await logoutFn(currentVal);
    });

    const concurrentLoginSequence = [];
    await page.route('**/api/v1/auth/login', async route => {
      const body = JSON.parse(route.request().postData() || '{}');
      const who = body.login;
      concurrentLoginSequence.push(`login-${who}-request`);
      if (who === 'series-admin') {
        await new Promise(r => setTimeout(r, 250));
      }
      const upstream = await route.fetch();
      concurrentLoginSequence.push(`login-${who}-response`);
      await route.fulfill({response: upstream});
    });

    const concurrentLoginResult = await page.evaluate(async () => {
      const chunkUrls = performance
        .getEntriesByType('resource')
        .map(r => r.name)
        .filter(u => u.includes('/_app/immutable/chunks/'));
      let loginFn, logoutFn, sessionStore;
      for (const url of chunkUrls) {
        const mod = await import(url);
        for (const val of Object.values(mod)) {
          if (typeof val === 'function' && val.toString().includes('/api/v1/auth/login')) loginFn = val;
          if (typeof val === 'function' && val.toString().includes('/api/v1/auth/logout')) logoutFn = val;
          if (val && typeof val === 'object' && typeof val.subscribe === 'function' && typeof val.set === 'function') sessionStore = val;
        }
        if (loginFn && logoutFn && sessionStore) break;
      }
      let currentVal = null;
      const unsub = sessionStore.subscribe(v => { currentVal = v; });

      // Start slow login A (series-admin, 250ms), then 20ms later start fast login B (series-user-b, 0ms)
      const pLoginA = loginFn('series-admin', 'disposable series browser password');
      await new Promise(r => setTimeout(r, 20));
      const pLoginB = loginFn('series-user-b', 'disposable series user-b password');
      await Promise.all([pLoginA, pLoginB]);
      unsub();

      const meRes = await fetch('/api/v1/auth/me', {credentials: 'same-origin', cache: 'no-store'});
      const meBody = meRes.ok ? await meRes.json() : null;

      // Also verify CSRF token from store works against the active HttpOnly cookie
      const logoutRes = await fetch('/api/v1/auth/logout', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {'x-csrf-token': currentVal.csrfToken}
      });
      return {
        storeSession: currentVal,
        meStatus: meRes.status,
        meBody,
        logoutStatus: logoutRes.status
      };
    });
    await page.unroute('**/api/v1/auth/login');

    assert.deepEqual(
      concurrentLoginSequence,
      [
        'login-series-admin-request',
        'login-series-admin-response',
        'login-series-user-b-request',
        'login-series-user-b-response'
      ],
      'Concurrent logins must be serialized so the slower first login Set-Cookie cannot arrive after and overwrite the second login cookie'
    );
    assert.equal(concurrentLoginResult.meStatus, 200);
    assert.equal(concurrentLoginResult.meBody.user.login, 'series-user-b');
    assert.equal(concurrentLoginResult.meBody.user.role, 'user');
    assert.equal(concurrentLoginResult.storeSession.user.login, 'series-user-b');
    assert.equal(concurrentLoginResult.storeSession.csrfToken, concurrentLoginResult.meBody.csrfToken);
    assert.equal(concurrentLoginResult.logoutStatus, 204);
    report.results.concurrentLoginsSequencedCorrectly = true;

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

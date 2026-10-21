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
    const context = await browser.newContext({viewport:{width:1440,height:900}, locale:'fr-FR'});
    const page = await context.newPage();
    const errors = [];
    const requestedMediaUrls = [];

    page.on('response', response => {
      if (response.url().includes('/api/v1/media/')) {
        requestedMediaUrls.push(response.url());
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

    // 2. Admin libraries: add Séries library, grant access to series-user-b in Users section, and scan
    await page.goto(origin + '/app/admin/libraries/');
    await page.getByRole('button', {name:'Ajouter une médiathèque', exact:true}).click();
    await page.locator('#library-name').fill('Séries Navigateur');
    await page.locator('#library-kind').selectOption('series');
    await page.getByRole('button', {name:'Enregistrer', exact:true}).click();
    await page.getByRole('heading', {name:'Séries Navigateur', exact:true}).waitFor();

    // Grant library access directly on series-user-b in Users section
    await page.locator('a[href="#users-section"]').click();
    const accounts = page.locator('#users-section');
    await accounts.getByRole('button', {name: 'Modifier series-user-b', exact: true}).click();
    await accounts.getByRole('button', {name: 'Accès', exact: true}).click();
    await page.locator('#users-section').getByLabel('Bibliothèque autorisée : Séries Navigateur').check();
    await page.locator('#users-section').getByRole('button', {name: 'Enregistrer', exact: true}).click();
    await page.getByText('Compte enregistré').waitFor();

    await page.locator('a[href="#libraries-section"]').click();
    await page.getByLabel('Actions Séries Navigateur', {exact:true}).click();
    await page.getByRole('button', {name:'Actualiser la médiathèque', exact:true}).click();
    await page.getByRole('dialog').getByRole('button', {name:'Actualiser', exact:true}).click();
    const article = page.locator('article').filter({has: page.getByRole('heading', {name:'Séries Navigateur', exact:true})});
    await article.getByText(/Terminé/).waitFor({timeout:60000});
    report.results.scan = true;
    await page.screenshot({path:`${out}/admin-scan.png`});

    await page.goto(origin+'/app/');
    await page.locator('.hero-banner h1').waitFor();
    await page.waitForFunction(()=>document.querySelector('.backdrop-img.loaded'));
    await page.waitForTimeout(550);
    for (const theme of ['light','dark']) {
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      await page.screenshot({path:`${out}/home-${theme}.png`});
    }
    // 3. Open Series catalogue
    await page.goto(origin + '/app/series/');
    const card = page.locator('.media-card').filter({hasText:'Browser Series'});
    await card.waitFor();
    await page.getByText('Page 1 sur 1',{exact:true}).waitFor();
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
    await page.getByRole('navigation',{name:'Saisons disponibles'}).waitFor();
    assert.equal(await page.locator('.main-nav a[href*="search"]').count(),0);
    assert.equal(await page.locator('main + footer').count(),0);
    assert.equal(await page.getByRole('link',{name:'Recherche rapide'}).count(),1);
    const favoriteSaved=page.waitForResponse(r=>r.url().includes('/api/v1/feedback/')&&r.request().method()==='PUT');
    await page.getByRole('button',{name:'Ajouter aux favoris',exact:true}).click();
    assert.equal((await favoriteSaved).status(),200);
    await page.getByRole('button',{name:'Retirer des favoris',exact:true}).waitFor();
    let releaseRating; const ratingGate=new Promise(resolve=>releaseRating=resolve); let ratingHeld=false;
    await page.route('**/api/v1/feedback/*',async route=>{if(route.request().method()==='PUT'){ratingHeld=true;await ratingGate;}await route.continue();});
    const firstRating=page.waitForRequest(request=>request.url().includes('/api/v1/feedback/')&&request.method()==='PUT');
    const ratingSaved=page.waitForResponse(r=>r.url().includes('/api/v1/feedback/')&&r.request().method()==='PUT'&&r.request().postDataJSON()?.rating===4.5);
    try {
      await page.getByRole('button',{name:'1 sur 5',exact:true}).click();
      await firstRating;
      await page.getByRole('button',{name:'4,5 sur 5',exact:true}).click();
      assert.equal(await page.getByRole('button',{name:'4,5 sur 5',exact:true}).getAttribute('aria-pressed'),'true');
      assert.equal(ratingHeld,true,'Rating updates immediately while the server response is held');
    } finally {releaseRating();await page.unrouteAll({behavior:'wait'});}
    assert.equal((await ratingSaved).status(),200);
    await page.reload();
    await page.getByRole('button',{name:'Retirer des favoris',exact:true}).waitFor();
    assert.equal(await page.getByRole('button',{name:'4,5 sur 5',exact:true}).getAttribute('aria-pressed'),'true');
    await page.locator('summary[aria-label="Ajouter à une liste"]').click();
    await page.getByLabel('Nouvelle liste',{exact:true}).fill('Sélection navigateur');
    const listCreated=page.waitForResponse(r=>r.url().endsWith('/api/v1/lists')&&r.request().method()==='POST');
    const listAdded=page.waitForResponse(r=>r.url().includes('/api/v1/lists/')&&r.url().includes('/items/')&&r.request().method()==='PUT');
    await page.getByRole('button',{name:'Créer et ajouter',exact:true}).click();
    const createdList=await listCreated;assert.equal(createdList.status(),200);const createdId=(await createdList.json()).id;
    assert.equal((await listAdded).status(),204);
    await page.getByRole('button',{name:'Sélection navigateur',exact:true}).waitFor();
    const listRead=await page.request.get(origin+'/api/v1/lists/'+encodeURIComponent(createdId));assert.equal(listRead.status(),200);
    assert.ok((await listRead.json()).items.length>0);
    await page.locator('summary[aria-label="Ajouter à une liste"]').click();
    report.results.quickFeedbackAndListMenu=true;
    await page.getByText('Synthetic Actor',{exact:true}).waitFor();
    await page.locator('.season-shortcuts img').first().scrollIntoViewIfNeeded();
    await page.waitForFunction(() => [...document.querySelectorAll('.season-shortcuts img')].some(img => img.complete && img.naturalWidth > 0));
    await page.locator('.episode-thumb').first().scrollIntoViewIfNeeded();
    await page.waitForFunction(() => [...document.querySelectorAll('.episode-thumb')].some(img => img.complete && img.naturalWidth > 0));
    report.results.localArtwork = true;
    for (const theme of ['light','dark']) {
      await page.evaluate(theme => document.documentElement.dataset.theme=theme, theme);
      for (const width of [390,1440]) {
        await page.setViewportSize({width,height:900});
        await page.evaluate(()=>window.scrollTo(0,0));
        await page.waitForTimeout(150);
        assert.ok(await page.getByRole('link',{name:'Recherche rapide'}).isVisible(),`search visible at ${width}`);
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth),false,`${theme} ${width} overflow`);
        const back=page.getByRole('link',{name:'Toutes les séries',exact:false});
        assert.equal(await back.count(),0);
        await page.screenshot({path:`${out}/detail-${theme}-${width}.png`,fullPage:true});
      }
    }
    await page.getByRole('navigation',{name:'Saisons disponibles'}).getByRole('button',{name:'Saison 2',exact:false}).click();
    await page.getByRole('button',{name:'Lire Season Two Episode',exact:true}).waitFor();
    await seasonSelect.selectOption('1');
    const singleVersionRow = page.locator('article.episode').filter({
      has: page.locator('.episode-number').filter({hasText:/^02$/})
    });
    await singleVersionRow.locator('h3').waitFor();
    assert.equal(await singleVersionRow.count(),1,'The second episode fixture must be present');
    assert.equal(await singleVersionRow.getByRole('combobox').count(),0,'A mono-version episode must not display a Version selector');
    report.results.catalogueChanges={privateFeedbackPersisted:true,seasonShortcuts:true,responsiveThemes:true};

    await page.screenshot({path:`${out}/series-detail.png`});

    // 5. Select Version and Play Episode 1
    const chapterVersion=page.getByRole('combobox',{name:'Version de Episode One'});
    await chapterVersion.focus();
    await chapterVersion.selectOption({label:'MP4'});
    const ep1PlayButton = page.getByRole('button', {name:'Lire Episode One', exact:true});
    await ep1PlayButton.waitFor();
    const launchStarted = Date.now();
    await ep1PlayButton.click();

    // Verify /api/v1/media/ was requested ONLY upon launch
    await page.waitForFunction(() => document.querySelector('video') !== null);
    await page.waitForFunction(() => document.querySelector('video')?.currentTime > 0.1);
    await page.waitForFunction(()=>{const video=document.querySelector('video');return video?.videoWidth>0&&(video.getVideoPlaybackQuality?.().totalVideoFrames??0)>0;});
    report.results.decodedVideoFrames=await page.evaluate(()=>document.querySelector('video').getVideoPlaybackQuality().totalVideoFrames);
    report.results.episode1Playback = true;
    report.results.episodeLaunchMs = Date.now() - launchStarted;
    const chapters=page.getByRole('combobox',{name:'Chapitres vidéo'});
    await chapters.waitFor();
    await chapters.selectOption('1');
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>=1);
    report.results.realChapters=true;

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
    const nextDescriptor = page.waitForResponse(response => response.url().includes('/api/v1/media/') && response.request().method()==='GET');
    await nextButton.click();
    await page.waitForFunction(title=>document.querySelector('.top-bar .title>span')?.textContent===title,nextTitle);
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>.1);
    const monoDescriptor = await (await nextDescriptor).json();
    const originalAudio = monoDescriptor.sources.find(source=>source.audioTrackId==='internal-default');
    assert.equal(originalAudio?.tracks.filter(track=>track.codec_type==='audio').length,1,'The next episode fixture has one detected internal audio track');
    assert.ok(monoDescriptor.sources.some(source=>/^internal-\d+$/.test(source.audioTrackId)),'The same track is also exposed in prepared packaging');
    assert.equal(await page.locator('dialog#player button[data-menu="audio"]').count(),0,'Original and remux of one audio track must not display an Audio choice');
    report.results.simplePlaybackControls={monoVersionSelectorHidden:true,monoAudioHidden:true,multipleVersionSelectorVisible:true};
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

    // Real Film detail exercises local NFO, track languages, subtitles and themes too.
    const seriesDetailUrl=page.url();
    const film = await page.evaluate(async root => {
      const me=await (await fetch('/api/v1/auth/me')).json();
      const headers={'content-type':'application/json','x-csrf-token':me.csrfToken};
      const response=await fetch('/api/v1/admin/libraries',{method:'POST',headers,body:JSON.stringify({name:'Film Fixture',kind:'films',nodeId:'local',root,enabled:true,userIds:[]})});
      if (!response.ok) throw new Error(`Film library HTTP ${response.status}`);
      const library=await response.json();
      const scan=await fetch(`/api/v1/admin/libraries/${library.id}/scan`,{method:'POST',headers,body:'{}'});
      if (!scan.ok) throw new Error('Film scan failed');
      return library;
    },process.env.MEDIA_TEST_FILM_ROOT);
    let filmScanCompleted = false;
    for (let i = 0; i < 120; i++) {
      const job = await page.evaluate(async id => {
        const response = await fetch(`/api/v1/admin/libraries/${id}/scan`);
        return await response.json();
      }, film.id);
      if (job.status === 'completed') {
        filmScanCompleted = true;
        break;
      }
      if (job.status === 'error') throw new Error(`Film scan error: ${JSON.stringify(job)}`);
      await page.waitForTimeout(500);
    }
    assert.ok(filmScanCompleted, 'Film scan did not complete in time');
    await page.goto(origin+'/app/films/');
    const filmCard = page.locator('.media-card').filter({hasText:'Synthetic Film'});
    await filmCard.waitFor();
    await filmCard.click();
    await page.getByRole('heading',{name:'Vidéo, audio et sous-titres'}).waitFor();
    await page.getByText('Film Actor',{exact:true}).waitFor();
    assert.match(await page.locator('.tech-specs-card').innerText(),/français/);
    assert.equal(await page.locator('.tech-specs-card .audio-track-spec').filter({hasText:'français'}).count(),1);
    assert.ok(await page.locator('.tech-specs-card .language-label svg').count() >= 2);
    assert.match(await page.locator('.tech-specs-card').innerText(),/anglais/);
    for (const theme of ['light','dark']) {
      await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      for (const width of [390,1440]) {
        await page.setViewportSize({width,height:900});
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth),false,`film ${theme} ${width} overflow`);
        await page.screenshot({path:`${out}/film-${theme}-${width}.png`,fullPage:true});
      }
    }
    report.results.filmPresentation=true;
    await page.goto(seriesDetailUrl);
    await seasonSelect.waitFor();

    // 9a. Real account switch in SPA (series-admin -> cache filled -> logout -> login series-user-b with real HttpOnly cookies)
    await seasonSelect.selectOption('1');
    const ep1Select = page.locator('select[aria-label="Version de Episode One"]');
    await ep1Select.waitFor();

    // Focus Episode 1 version selector in Session A to ensure all its versions are prefetched and cached
    await ep1Select.focus();
    await page.waitForFunction(
      () => {
        const sel = document.querySelector('select[aria-label="Version de Episode One"]');
        return sel && Array.from(sel.options).filter(opt=>opt.value!=='auto').every(opt => opt.textContent === 'MP4' || opt.textContent === 'MKV');
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
    await page.getByRole('button', {name: /^(Se déconnecter|Sign out)$/, exact: true}).click();
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
    await page.getByRole('navigation', {name: /^(Navigation principale|Main navigation)$/}).getByRole('link', {name: 'Séries Navigateur', exact: true}).click();
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
    const initialLabelsB = await ep1SelectB.locator('option:not([value="auto"])').allTextContents();
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
        return sel && Array.from(sel.options).filter(opt=>opt.value!=='auto').every(opt => opt.textContent === 'MP4' || opt.textContent === 'MKV');
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

    // Personal security UI: change the current password, revoke another live device and revoke all.
    await page.goto(origin+'/app/settings/#account');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('series-user-b');
    await page.locator('input[autocomplete="current-password"]').fill('disposable series user-b password');
    await page.locator('button[type="submit"]').click();await page.waitForURL(origin+'/app/');
    const otherDevice=await browser.newContext();
    const loginOther=await otherDevice.request.post(origin+'/api/v1/auth/login',{headers:{origin},data:{login:'series-user-b',password:'disposable series user-b password'}});assert.equal(loginOther.status(),200);
    await page.goto(origin+'/app/settings/#account');await page.getByRole('heading',{name:'Appareils connectés'}).waitFor();
    for(const width of [390,1440]){await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await page.screenshot({path:`${out}/account-security-${width}.png`,fullPage:true});}
    await page.getByLabel('Mot de passe actuel',{exact:true}).fill('disposable series user-b password');
    await page.getByLabel('Nouveau mot de passe',{exact:true}).fill('changed disposable user-b password');
    await page.getByLabel('Confirmer le nouveau mot de passe',{exact:true}).fill('changed disposable user-b password');
    await page.getByRole('button',{name:'Changer le mot de passe',exact:true}).click();await page.waitForURL('**/app/login/**');
    assert.equal((await otherDevice.request.get(origin+'/api/v1/auth/me')).status(),401);
    await otherDevice.close();
    await page.locator('input[autocomplete="username"]').fill('series-user-b');
    await page.locator('input[autocomplete="current-password"]').fill('changed disposable user-b password');await page.locator('button[type="submit"]').click();await page.waitForURL(origin+'/app/');
    await page.goto(origin+'/app/settings/#account');
    await page.getByRole('button',{name:'Déconnecter tous les appareils',exact:true}).click();await page.waitForURL('**/app/login/**');
    report.results.personalPasswordAndDeviceRevocation=true;
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
    if (out === 'docs/evidence/2026-10-17/series') {
      try {
        fs.mkdirSync('docs/evidence/2026-10-19/catalogue', { recursive: true });
        fs.cpSync(out, 'docs/evidence/2026-10-19/catalogue', { recursive: true });
      } catch {}
    }
    console.log(JSON.stringify(report));
  }
})();

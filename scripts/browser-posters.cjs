const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const phase = process.env.MEDIA_TEST_PHASE || 'initial';
const expectedTitle = 'NFO Film & <img src=x onerror=alert(1)>';
const expectedSynopsis = 'Texte & <script>alert(1)</script>';
const out = process.env.MEDIA_REPORT_DIR || 'docs/evidence/2026-10-16/posters';
fs.mkdirSync(out, {recursive:true});
(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  const report = {browser:browser.version(),os:require('node:os').release(),headless:true,phase,results:{}};
  try {
    const context = await browser.newContext({viewport:{width:1440,height:900}});
    const page = await context.newPage();
    const errors=[];
    page.on('pageerror',error=>errors.push(String(error)));
    page.on('console',message=>{if(message.type()==='error' && /Content Security Policy/i.test(message.text())) errors.push(message.text());});
    await page.goto(origin+'/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('scanner-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable scanner browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin+'/app/');
    if (phase !== 'restart') {
      await page.goto(origin+'/app/admin/libraries/');
      if (phase === 'initial') {
        await page.getByRole('button',{name:'Ajouter une médiathèque',exact:true}).click();
        await page.getByLabel('Nom de la bibliothèque',{exact:true}).fill('Films scanner navigateur');
        await page.getByLabel('Racine autorisée sur le Node',{exact:true}).selectOption({index:1});
        await page.getByRole('dialog').getByRole('button',{name:'Enregistrer',exact:true}).click();
        await page.getByRole('heading',{name:'Films scanner navigateur',exact:true}).waitFor();
      }
      const article=page.locator('article').filter({has:page.getByRole('heading',{name:'Films scanner navigateur',exact:true})});
      const runScan = async () => {
        const posted=page.waitForResponse(r=>r.request().method()==='POST' && r.url().endsWith('/scan'));
        await page.getByLabel('Actions Films scanner navigateur',{exact:true}).click();
      await page.getByRole('button',{name:'Actualiser la médiathèque',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
        assert.equal((await posted).status(),202);
        await page.waitForFunction(async()=>{
          const data=await (await fetch('/api/v1/admin/libraries')).json();
          const library=data.libraries.find(l=>l.name==='Films scanner navigateur');
          const scan=await (await fetch('/api/v1/admin/libraries/'+library.id+'/scan')).json();
          return scan.status==='completed';
        },null,{timeout:60000});
        await page.reload();
      };
      await runScan();
      await article.getByText(/Scan : .*2 fichier/).waitFor({timeout:60000});
      if (phase === 'initial') await runScan();
      await article.getByText(/Scan : .*0 analys.*2 r.utilis/).waitFor({timeout:60000});
      report.results.incrementalWithoutVideoProbe=true;
    }
    if (phase === 'initial') {
      report.results.progressiveImages = [];
      for (const width of [1440, 390]) {
        await page.setViewportSize({width, height:900});
        let release;
        let held = false;
        const gate = new Promise(resolve => { release = resolve; });
        const handler = async route => {
          if (new URL(route.request().url()).searchParams.get('kind') === 'series') {
            held = true;
            await gate;
          }
          await route.continue();
        };
        await page.route('**/api/v1/catalog?*', handler);
        const start = performance.now();
        try {
          await page.goto(origin+'/app/');
          await page.waitForFunction(() => {
            const img = document.querySelector('a.media-card img.poster-image');
            return img?.complete && img.naturalWidth > 0 && Number(getComputedStyle(img).opacity) === 1;
          });
          assert.equal(held, true, 'Series request must still be pending when film poster is visible');
          await page.getByText('Chargement du reste du catalogue…', {exact:true}).waitFor();
          assert.equal(await page.locator('a.media-card img.poster-image').first().getAttribute('loading'), 'eager');
          report.results.progressiveImages.push({width, firstPosterMs:Math.round(performance.now()-start), slowerShelfStillPending:true});
          await page.screenshot({path:`${out}/progressive-${width}.png`,fullPage:true});
        } finally {
          release();
          await page.unrouteAll({behavior:'wait'});
        }
        await page.getByText('Chargement du reste du catalogue…', {exact:true}).waitFor({state:'hidden'});
      }
      await page.setViewportSize({width:1440,height:900});
      const failSeries = async route => {
        if (new URL(route.request().url()).searchParams.get('kind') === 'series') {
          await route.fulfill({status:503,contentType:'application/json',body:'{"error":"synthetic_unavailable"}'});
        } else await route.continue();
      };
      await page.route('**/api/v1/catalog?*',failSeries);
      try {
        await page.goto(origin+'/app/');
        await page.getByText('Une partie du catalogue est indisponible. Rechargez la page pour réessayer.',{exact:true}).waitFor();
        await page.locator('a.media-card').filter({hasText:expectedTitle}).waitFor();
        report.results.partialCatalogueFailureKeepsFilms = true;
      } finally { await page.unrouteAll({behavior:'wait'}); }
    }
    for (const route of ['/app/','/app/search/?q='+encodeURIComponent(expectedTitle),'/app/films/']) {
      await page.goto(origin+route);
      const card=page.locator('a.media-card').filter({hasText:expectedTitle});
      await card.waitFor();
      assert.equal(await page.locator('a.media-card').count(),1);
      if (phase === 'delete') {
        assert.equal(await card.locator('img.poster-image').count(),0);
      } else {
        await card.locator('img.poster-image').scrollIntoViewIfNeeded();
        await page.waitForFunction(() => {const img=document.querySelector('a.media-card img.poster-image');return img?.complete && img.naturalWidth > 0;});
      }
      {
        assert.match(await card.textContent(),/2025/);
        assert.match(await card.textContent(),/Drame/);
        assert.ok((await card.textContent()).includes(expectedSynopsis));
      }
    }
    const poster = page.locator('a.media-card img.poster-image');
    if (phase === 'delete') {
      assert.equal(await poster.count(), 0);
      await page.locator('a.media-card .poster-type').waitFor();
    } else {
      await poster.scrollIntoViewIfNeeded();
      await page.waitForFunction(() => {const img=document.querySelector('a.media-card img.poster-image');return img?.complete && img.naturalWidth > 0;});
      assert.equal(await poster.getAttribute('loading'),'eager');
      const imageUrl=await poster.getAttribute('src');
      assert.equal(await poster.evaluate(img=>img.naturalWidth),phase==='update'?3:2);
      if(phase==='initial') fs.writeFileSync(out+'/initial-image-url.txt',imageUrl);
      if(phase==='restart') assert.equal(imageUrl,fs.readFileSync(out+'/initial-image-url.txt','utf8'));
      if(phase==='update') assert.notEqual(imageUrl,fs.readFileSync(out+'/initial-image-url.txt','utf8'));
      const response=await context.request.get(origin+imageUrl);
      assert.equal(response.status(),200);
      assert.equal(response.headers()['content-type'],'image/png');
      assert.equal(response.headers()['x-content-type-options'],'nosniff');
      assert.match(response.headers()['cache-control'],/private, no-cache/);
      assert.equal((await context.request.head(origin+imageUrl)).status(),200);
      const anonymous=await browser.newContext();
      assert.equal((await anonymous.request.get(origin+imageUrl)).status(),401);
      assert.equal((await anonymous.request.head(origin+imageUrl)).status(),401);
      await anonymous.close();
      await page.route('**/api/v1/assets/*/image*',route=>route.abort());
      await page.reload();
      await page.locator('a.media-card .poster-type').waitFor();
      await page.unroute('**/api/v1/assets/*/image*');
      await page.reload();
      await page.locator('a.media-card img.poster-image').waitFor();
    }
    await page.screenshot({path:`${out}/catalog-${phase}.png`});
    await page.locator('a.media-card').click();
    await page.getByRole('heading',{name:expectedTitle,exact:true}).waitFor();
    {
      assert.equal(await page.locator('.synopsis').textContent(),expectedSynopsis);
      assert.match(await page.locator('.film-detail-view .genres-row').textContent(),/Drame/);
      assert.match(await page.locator('.detail-info-column .badge-row').textContent(),/2025/);
      assert.equal(await page.locator('.detail-info-column script').count(),0);
    }
    if (phase !== 'delete') {
      await page.locator('.detail-poster-column img.poster-image').waitFor();
      await page.waitForFunction(width=>{const img=document.querySelector('.detail-poster-column img.poster-image');return img?.complete && img.naturalWidth===width;},phase==='update'?3:2);
    }
    await page.screenshot({path:`${out}/detail-${phase}.png`});
    await page.getByRole('button',{name:'Sélectionner la version MP4',exact:true}).click();
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
    await page.evaluate(()=>{const video=document.querySelector('video');video.pause();video.currentTime=4;});
    await page.waitForFunction(()=>{const video=document.querySelector('video');return !video.seeking && video.paused && Math.abs(video.currentTime-4)<0.3;});
    await page.screenshot({path:`${out}/playback-${phase}.png`});
    assert.deepEqual(errors,[]);
    report.results={...report.results,catalogue:true,singleWorkTwoVersions:true,metadata:true,localPoster:true,fallback:true,imageSessionPermissions:true,escapedHostileText:true,playback:true,seek:true,pause:true,cspErrors:errors};
    report.passed=true;
    await context.close();
  } catch(error) {report.failure=String(error.stack);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report-${phase}.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

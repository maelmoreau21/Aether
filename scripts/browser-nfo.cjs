const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const phase = process.env.MEDIA_TEST_PHASE || 'initial';
const expectedTitle = phase === 'delete' ? 'Scanner Film' : phase === 'update' ? 'NFO Film actualise' : 'NFO Film & <img src=x onerror=alert(1)>';
const expectedSynopsis = phase === 'update' ? 'Synopsis actualise' : 'Texte & <script>alert(1)</script>';
const out = 'docs/evidence/2026-10-16/nfo';
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
        await page.getByRole('button',{name:'Ajouter une bibliothèque',exact:true}).click();
        await page.getByLabel('Nom de la bibliothèque',{exact:true}).fill('Films scanner navigateur');
        await page.getByRole('button', {name:'Ajouter ce dossier', exact:true}).click();
        await page.getByRole('dialog').getByRole('button',{name:'Enregistrer',exact:true}).click();
        await page.getByRole('heading',{name:'Films scanner navigateur',exact:true}).waitFor();
      }
      const article=page.locator('article').filter({has:page.getByRole('heading',{name:'Films scanner navigateur',exact:true})});
      await page.getByLabel('Actions Films scanner navigateur',{exact:true}).click();
      await page.getByRole('button',{name:'Actualiser la bibliothèque',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
      await article.getByText(/Terminé/).waitFor({timeout:60000});
      assert.match(await article.textContent(),/2 fichier\(s\) découvert\(s\)/);
      if (phase === 'initial') {
        await page.getByLabel('Actions Films scanner navigateur',{exact:true}).click();
      await page.getByRole('button',{name:'Actualiser la bibliothèque',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
      }
      await article.getByText(/Scan : Terminé.*0 analysé\(s\).*2 réutilisé\(s\)/).waitFor({timeout:60000});
      report.results.incrementalWithoutVideoProbe=true;
    }
    for (const route of ['/app/','/app/search/?q='+encodeURIComponent(expectedTitle),'/app/films/']) {
      await page.goto(origin+route);
      const card=page.locator('a.media-card').filter({hasText:expectedTitle});
      await card.waitFor();
      assert.equal(await page.locator('a.media-card').count(),1);
      if (phase !== 'delete') {
        assert.match(await card.textContent(),/2025/);
        assert.match(await card.textContent(),/Drame/);
        assert.ok((await card.textContent()).includes(expectedSynopsis));
      }
    }
    await page.locator('a.media-card').click();
    await page.getByRole('heading',{name:expectedTitle,exact:true}).waitFor();
    if (phase !== 'delete') {
      assert.equal(await page.locator('.synopsis').textContent(),expectedSynopsis);
      assert.match(await page.locator('.genres').textContent(),/Drame/);
      assert.match(await page.locator('.detail .metadata').first().textContent(),/2025/);
      assert.equal(await page.locator('.detail-copy script, .detail-copy img').count(),0);
    }
    await page.screenshot({path:`${out}/detail-${phase}.png`});
    await page.getByRole('button',{name:'Sélectionner la version MP4',exact:true}).click();
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
    await page.evaluate(()=>{const video=document.querySelector('video');video.pause();video.currentTime=4;});
    await page.waitForFunction(()=>{const video=document.querySelector('video');return !video.seeking && video.paused && Math.abs(video.currentTime-4)<0.3;});
    await page.screenshot({path:`${out}/playback-${phase}.png`});
    assert.deepEqual(errors,[]);
    report.results={...report.results,catalogue:true,singleWorkTwoVersions:true,metadata:true,escapedHostileText:true,playback:true,seek:true,pause:true,cspErrors:errors};
    report.passed=true;
    await context.close();
  } catch(error) {report.failure=String(error.stack);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report-${phase}.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

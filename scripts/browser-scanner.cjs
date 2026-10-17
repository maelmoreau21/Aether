const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const restart = process.env.MEDIA_TEST_RESTART === '1';
const out = process.env.MEDIA_REPORT_DIR || 'docs/evidence/2026-10-16/scanner';
if (!restart) fs.rmSync(out, {recursive: true, force: true});
fs.mkdirSync(out, {recursive: true});
(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  const report = {browser:browser.version(), os:require('node:os').release(), headless:true, restart, results:{}};
  try {
    const context = await browser.newContext({viewport:{width:1440,height:900}});
    const page = await context.newPage();
    const errors=[];
    page.on('response', async response=>{if(response.url().endsWith('/scan')) console.log('scan HTTP',response.status(),await response.text());});
    page.on('pageerror', error=>errors.push(String(error)));
    page.on('console', message=>{if(message.type()==='error' && /Content Security Policy/i.test(message.text())) errors.push(message.text());});
    await page.goto(origin+'/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('scanner-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable scanner browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin+'/app/');
    if (!restart) {
      await page.goto(origin+'/app/admin/libraries/');
      await page.getByLabel('Nom',{exact:true}).fill('Films scanner navigateur');
      await page.getByRole('button',{name:'Enregistrer',exact:true}).click();
      await page.getByRole('heading',{name:'Films scanner navigateur',exact:true}).waitFor();
      await page.getByRole('button',{name:'Scanner Films scanner navigateur',exact:true}).click();
      const article = page.locator('article').filter({has:page.getByRole('heading',{name:'Films scanner navigateur',exact:true})});
      await article.getByText(/Scan : Terminé/).waitFor({timeout:60000});
      assert.match(await article.textContent(), /2 fichier\(s\) découvert\(s\)/);
      report.results.scan = true;
      await page.screenshot({path:`${out}/scan.png`});
      await page.getByRole('button',{name:'Scanner Films scanner navigateur',exact:true}).click();
      await article.getByText(/Scan : Terminé.*0 analysé\(s\).*2 réutilisé\(s\)/).waitFor({timeout:60000});
      report.results.incremental = true;
    }
    await page.goto(origin+'/app/films/');
    const card=page.locator('.media-card').filter({hasText:'Scanner Film'});
    await card.waitFor();
    assert.equal(await page.locator('.media-card').count(), 1);
    await card.click();
    await page.getByRole('heading',{name:'Scanner Film',exact:true}).waitFor();
    await page.getByRole('button',{name:'Sélectionner la version MP4',exact:true}).click();
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
    await page.evaluate(()=>{const video=document.querySelector('video');video.pause();video.currentTime=4;});
    await page.waitForFunction(()=>{const video=document.querySelector('video');return !video.seeking && video.paused && Math.abs(video.currentTime-4)<0.3;});
    await page.screenshot({path:`${out}/playback${restart?'-restart':''}.png`});
    assert.deepEqual(errors, []);
    report.results = {...report.results,catalogue:true,singleWorkTwoVersions:true,playback:true,seek:true,pause:true,cspErrors:errors};
    report.passed=true;
    await context.close();
  } catch(error) {report.failure=String(error.stack);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report${restart?'-restart':''}.json`, JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

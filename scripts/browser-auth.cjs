const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const out = 'docs/evidence/2026-10-16/auth';
fs.mkdirSync(out, {recursive:true});
(async () => {
  const browser = await chromium.launch({channel:'chrome',headless:true});
  const report = {browser:browser.version(),os:require('node:os').release(),headless:true,results:[]};
  try {
    for (const width of [390,1440]) {
      const context = await browser.newContext({viewport:{width,height:900}});
      const page = await context.newPage();
      const errors=[];
      page.on('pageerror', e=>errors.push(String(e)));
      page.on('console',m=>{if(m.type()==='error' && /Content Security Policy/i.test(m.text())) errors.push(m.text());});
      await page.goto(origin+'/app/');
      await page.waitForURL('**/app/login/**');
      await page.locator('input[autocomplete="username"]').fill('browser-admin');
      await page.locator('input[autocomplete="current-password"]').fill('incorrect password');
      await page.locator('button[type="submit"]').click();
      await page.getByRole('alert').waitFor();
      await page.locator('input[autocomplete="current-password"]').fill('disposable browser fixture password');
      await page.screenshot({path:`${out}/login-${width}.png`});
      await page.locator('button[type="submit"]').click();
      await page.locator('.media-card').first().waitFor();
      const cookie=(await context.cookies()).find(c=>c.name==='media_session');
      assert(cookie && cookie.httpOnly && cookie.sameSite==='Strict' && !cookie.secure);
      assert.equal(await page.evaluate(()=>document.cookie.includes('media_session')),false);
      assert.equal(await page.evaluate(()=>Object.keys(localStorage).some(k=>/session|token|auth/i.test(k))),false);
      await page.getByRole('button',{name:'Menu profil',exact:true}).click();
      const menu=page.getByRole('navigation',{name:'Menu profil',exact:true});
      assert.match(await page.locator('#profile-menu').textContent(),/browser-admin/);
      assert.match(await menu.textContent(),/Administration/);
      await page.keyboard.press('Escape');
      await page.goto(origin+'/app/films/?work=fixture-film');
      await page.locator('.version-choice').first().waitFor();
      await page.getByRole('button',{name:'Sélectionner la version MP4'}).click();
      await page.locator('.hero-cta').click();
      await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
      await page.evaluate(()=>{const v=document.querySelector('video');v.pause();v.currentTime=4;});
      await page.waitForFunction(()=>{const v=document.querySelector('video');return !v.seeking && Math.abs(v.currentTime-4)<0.3;});
      await page.getByRole('button',{name:'Retour à l’accueil'}).click();
      await page.reload(); await page.locator('.version-choice').first().waitFor();
      await page.getByRole('button',{name:'Menu profil',exact:true}).click();
      await page.getByRole('button',{name:'Se déconnecter',exact:true}).click();
      await page.waitForURL('**/app/login/**');
      assert.equal((await context.request.get(origin+'/api/v1/catalog')).status(),401);
      assert.equal((await context.request.get(origin+'/api/v1/assets/demo-mp4-fr/content',{headers:{Range:'bytes=0-31'}})).status(),401);
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      assert.deepEqual(errors,[]);
      report.results.push({width,invalidLogin:true,login:true,httpOnlyCookie:true,avatarIdentity:true,administratorMenu:true,catalog:true,playback:true,seek:true,reload:true,logout:true,anonymousRange401:true,cspErrors:errors});
      await context.close();
    }
    report.passed=true;
  } catch(e) {report.failure=String(e.stack);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();


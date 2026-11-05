// Production UI regression with synthetic account, request and artwork APIs.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5187/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/profile-artwork-fixes';
fs.mkdirSync(out, { recursive: true });
(async () => {
  const browser = await chromium.launch({channel:'chrome',headless:true});
  try {
    for (const width of [390, 1920]) {
      const {page, context, errors} = await fixturePage(browser, width, 'dark');
      let searches = 0;
      await page.route('**/api/v1/extensions/features', route => route.fulfill({json:{requests:true,reports:true}}));
      await page.route('**/api/v1/requests**', async route => {
        const path = new URL(route.request().url()).pathname;
        if (path.endsWith('/search')) {
          searches++;
          await new Promise(resolve => setTimeout(resolve, 250));
          return route.fulfill({json:{results:[{kind:'film',title:'Film de test',externalId:'1',externalProvider:'tmdb'}]}});
        }
        return route.fulfill({json:path.endsWith('/destinations')?{servers:[]}:{requests:[]}});
      });
      await page.goto(`${base}settings/#requests`);
      const button = page.locator('.application-page button.primary').first();
      await button.waitFor();
      assert.equal(await button.isEnabled(), true);
      assert.equal(await button.evaluate(el => getComputedStyle(el).cursor), 'pointer');
      await button.click();
      assert.equal(searches, 0, 'empty search uses native field validation');
      await page.locator('input[type="search"]').fill('Film');
      await button.hover();
      assert.equal(searches, 0, 'hover must not submit');
      await button.click();
      await page.getByRole('heading',{name:'Film de test',exact:true}).waitFor();
      assert.equal(searches, 1);
      assert.equal(await button.isEnabled(), true);
      await page.screenshot({path:`${out}/requests-${width}.png`});
      for (const [name, w, h] of [['wide',2400,600],['portrait',600,900],['square',900,900]]) {
        await page.route('**/ux-art/adaptive', route => route.fulfill({contentType:'image/svg+xml',body:`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#426b81"/><text x="20" y="100" fill="white" font-size="50">Image ${name}</text></svg>`}));
        await page.route('**/api/v1/catalog/**', route => route.fulfill({json:{id:'film-0',kind:'film',title:'Titre français',libraryId:'film',year:2026,description:'Synopsis de test.',synthetic:true,editions:[],artwork:{backdrop:'/ux-art/adaptive',logo:'/ux-art/logo',illustration:'/ux-art/poster-1'}}}));
        await page.goto(`${base}films/?work=film-0`);
        await page.locator('.detail .backdrop:not(.artwork-fill)').waitFor();
        await page.waitForFunction(() => [...document.querySelectorAll('.detail .backdrop, .detail .title-logo')].every(img=>img.complete));
        assert.equal(await page.locator('.detail h1').innerText(), 'Titre français');
        assert.equal(await page.locator('.detail .clearart').count(),0,'no clearart on top of a scene');
        const img=page.locator('.detail .backdrop:not(.artwork-fill)');
        const fit=await img.evaluate(el=>({fit:getComputedStyle(el).objectFit,box:el.clientWidth/el.clientHeight,ratio:el.naturalWidth/el.naturalHeight,width:el.naturalWidth}));
        assert.equal(fit.width,w,'the requested artwork fixture was decoded');
        if (1-Math.min(fit.box,fit.ratio)/Math.max(fit.box,fit.ratio)>0.08) assert.equal(fit.fit,'contain');
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
        await page.screenshot({path:`${out}/${name}-${width}.png`});
      }
      await page.goto(`${base}admin/libraries/#avatars-section`);
      await page.getByRole('heading',{name:'Votre galerie d’avatars'}).waitFor();
      assert.equal(await page.getByText('Importer un pack depuis GitHub',{exact:true}).count(),0);
      assert.equal(await page.locator('input[type="file"]').count(),1);
      assert.deepEqual(errors,[]);
      await context.close();
    }
    console.log('Passed: request hover/click/native validation; three artwork ratios at 390/1920 px.');
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});

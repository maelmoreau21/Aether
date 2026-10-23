// Real Chrome/Svelte UI, delayed API fixtures; no NAS or backend timing claim.
const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  try {
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', error => errors.push(String(error)));
    let preferencesFinished = false, seriesFinished = false;
    const requests = [];
    await page.route('**/api/v1/**', async route => {
      const url = new URL(route.request().url());
      let data = {};
      if (url.pathname === '/api/v1/auth/me') data = {user:{id:'progressive',login:'progressive',role:'user'},csrfToken:'fixture'};
      else if (url.pathname === '/api/v1/preferences') { await delay(2500); preferencesFinished = true; data = {locale:'fr'}; }
      else if (url.pathname === '/api/v1/libraries') data = {libraries:[]};
      else if (url.pathname === '/api/v1/catalog') {
        requests.push(url.search);
        assert.equal(url.searchParams.get('limit'), '24');
        const kind = url.searchParams.get('kind');
        await delay(kind === 'series' ? 1800 : 100);
        if (kind === 'series') seriesFinished = true;
        data = {items:Array.from({length:24}, (_,i) => ({id:`${kind}-${i}`,kind,title:`${kind} ${i}`,year:2026,libraryId:'fixture',synthetic:true,editions:[]})),total:null,hasMore:true,offset:0,limit:24,notice:''};
      } else if (url.pathname.includes('progress')) data = {items:[]};
      await route.fulfill({json:data}).catch(() => {});
    });
    await page.goto('http://127.0.0.1:5179/app/');
    await page.locator('.catalog-home .media-card').first().waitFor();
    assert.equal(preferencesFinished, false, 'Catalogue must not wait for preferences');
    assert.equal(seriesFinished, false, 'Films must not wait for series');
    assert.equal(await page.locator('.catalog-home .media-card').count(),24);
    await page.waitForFunction(() => document.querySelectorAll('.catalog-home .media-card').length >= 48);
    assert.equal(requests.length,2);
    assert(requests.every(query => new URLSearchParams(query).get('count') === 'false'));
    assert.deepEqual(errors,[]);
    const report = {browser:browser.version(),scope:'UI with delayed API fixtures, 24 cards per kind; actual 10k backend tested separately',checks:['film shelf before preferences','film shelf before series','two bounded requests without global count','second shelf arrives progressively'],requests};
    fs.mkdirSync('target/navigation-performance',{recursive:true});
    fs.writeFileSync('target/navigation-performance/progressive.json',JSON.stringify(report,null,2));
    console.log(JSON.stringify(report,null,2));
  } finally { await browser.close(); }
})().catch(error => {console.error(error);process.exitCode=1;});

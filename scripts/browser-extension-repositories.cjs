// Real Svelte UI with synthetic intercepted APIs; no remote repository is used.
const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN || 'http://127.0.0.1:5179';
const manifest = id => ({id,name:id,version:'1.0.0',description:'Synthetic repository fixture',author:'Test',permissions:[]});
async function run() {
  const browser = await chromium.launch({channel:'chrome',headless:true});
  const report = [];
  try {
    for (const width of [390,1440]) {
      const {page,context,errors} = await setup(browser,width,'dark');
      const sources = [
        {id:'stable',name:'Stable fixture',url:'https://example.com/stable.json',refreshedAt:1},
        {id:'offline',name:'Offline fixture',url:'https://example.com/offline.json',refreshedAt:1}
      ];
      const refreshed = [], writes = [];
      let added;
      await page.route('**/api/v1/admin/extensions/**',async route => {
        const request = route.request(), path = new URL(request.url()).pathname;
        if (request.method() !== 'GET') writes.push(path);
        if (path === '/api/v1/admin/extensions/repositories') {
          if (request.method() === 'POST') {
            added = request.postDataJSON();
            sources.push({id:'develop',...added,refreshedAt:1});
            return route.fulfill({status:201,json:{id:'develop'}});
          }
          return route.fulfill({json:{repositories:sources}});
        }
        if (path.endsWith('/refresh')) {
          const id = path.split('/').at(-2); refreshed.push(id);
          return route.fulfill(id === 'offline' ? {status:502,json:{error:'repository_unavailable'}} : {status:204});
        }
        if (path.endsWith('/catalogue')) return route.fulfill({json:{entries:[
          {repositoryId:'stable',repositoryName:'Stable fixture',compatible:true,manifest:manifest(refreshed.includes('stable')?'Fresh extension':'Old extension'),publisherId:'test',publicKey:'test'},
          {repositoryId:'offline',repositoryName:'Offline fixture',compatible:true,manifest:manifest('Cached extension'),publisherId:'test',publicKey:'test'}
        ]}});
        return route.fulfill({json:{}});
      });
      await page.goto(`${origin}/app/admin/libraries/#extensions-section`);
      const panel = page.locator('#extensions-section');
      await panel.getByRole('alert').filter({hasText:'Offline fixture'}).waitFor();
      assert.deepEqual(refreshed.sort(),['offline','stable']);
      await panel.locator('.extension-tabs button').nth(1).click();
      await panel.getByRole('heading',{name:'Fresh extension',exact:true}).waitFor();
      await panel.getByRole('heading',{name:'Cached extension',exact:true}).waitFor();
      assert.equal(writes.filter(path=>path.endsWith('/install')).length,0);
      await panel.locator('.extension-tabs button').nth(2).click();
      await panel.locator('.section-title button').click();
      await panel.getByRole('button',{name:'Aether Plugins · develop',exact:true}).click();
      assert.equal(await panel.locator('input[type=url]').inputValue(),'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/develop/repository/manifest.json');
      await panel.locator('.repository-form button').last().click();
      await page.waitForFunction(()=>!document.querySelector('.repository-form'));
      assert.equal(added.name,'Aether Plugins (develop)');
      assert.equal(added.url,'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/develop/repository/manifest.json');
      refreshed.length = 0;
      await page.reload();
      await panel.getByRole('alert').filter({hasText:'Offline fixture'}).waitFor();
      assert.deepEqual(refreshed.sort(),['develop','offline','stable']);
      assert.equal(writes.filter(path=>path.endsWith('/install')).length,0);
      assert.deepEqual(errors,[]);
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      report.push({width,automaticRefresh:true,offlineCachePreserved:true,developPreset:true,noAutomaticInstall:true});
      await context.close();
    }
  } finally { await browser.close(); }
  fs.mkdirSync('target/extension-repositories',{recursive:true});
  fs.writeFileSync('target/extension-repositories/report.json',JSON.stringify(report,null,2));
  console.log(JSON.stringify(report,null,2));
}
run().catch(error=>{console.error(error);process.exitCode=1;});

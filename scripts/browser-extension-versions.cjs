const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs'); fs.mkdirSync('output/playwright/extension-versions', {recursive:true});
const manifest = (v) => ({ id:'media-requests', name:'Demandes', version:v, description:'Extension de test', author:'Test', permissions:[] });
async function run() {
  const browser=await chromium.launch({channel:'chrome',headless:true});
  try { for (const width of [390,1440]) {
    const {page,context,errors}=await setup(browser,width,'dark');
    const ext={manifest:manifest('1.0.3'),publisherId:'pub',sha256:'digest',enabled:true,libraryIds:[],rollbackVersion:'1.0.1'};
    const catalogue=[{repositoryId:'repo',repositoryName:'Test',compatible:true,manifest:manifest('1.0.3')},{repositoryId:'repo',repositoryName:'Test',compatible:true,manifest:manifest('1.0.0')}];
    await page.route('**/api/v1/admin/extensions',r=>r.fulfill({json:{extensions:[ext]}}));
    await page.route('**/api/v1/admin/extensions/catalogue',r=>r.fulfill({json:{entries:catalogue}}));
    await page.route('**/api/v1/admin/extensions/repositories',r=>r.fulfill({json:{repositories:[]}}));
    await page.route('**/api/v1/extensions/features',r=>r.fulfill({json:{requests:true,reports:false}}));
    await page.goto('http://127.0.0.1:5192/app/admin/libraries/#extensions-section');
    const card=page.getByRole('button',{name:'Ouvrir Demandes',exact:true}); await card.waitFor();
    assert.equal(await card.getByText('Paramètres').count(),0); await card.focus(); await page.keyboard.press('Enter');
    await page.getByRole('heading',{name:'Versions',exact:true}).waitFor();
    assert.equal(await page.getByText('Historique des révisions',{exact:true}).count(),0);
    const row=page.locator('.revision-item').filter({hasText:'1.0.1'}); await row.getByRole('button',{name:/1\.0\.1/}).click();
    await row.getByRole('checkbox',{name:/Je confirme/}).check(); assert.equal(await row.getByRole('button',{name:'Restaurer v1.0.1'}).isEnabled(),true);
    await page.getByRole('button',{name:'Épingler dans le menu',exact:true}).click();
    await page.getByRole('button',{name:'Désépingler du menu',exact:true}).waitFor();
    const old=page.locator('.revision-item').filter({hasText:'1.0.0'}); await old.getByRole('button',{name:/1\.0\.0/}).click();
    await old.getByRole('checkbox',{name:/Je confirme/}).check();
    assert.equal(await old.getByRole('button',{name:'Installer v1.0.0'}).isDisabled(),true);
    await page.screenshot({path:`output/playwright/extension-versions/detail-${width}.png`,fullPage:true});
    ext.rollbackVersion=null; await page.reload();
    await page.getByRole('button',{name:'Ouvrir Demandes',exact:true}).click();
    const available=page.locator('.revision-item').filter({hasText:'1.0.0'}); await available.getByRole('button',{name:/1\.0\.0/}).click();
    await available.getByRole('checkbox',{name:/Je confirme/}).check();
    let sent; await page.route('**/api/v1/admin/extensions/repositories/repo/install',route=>{sent=route.request().postDataJSON();return route.fulfill({status:201,json:{id:'media-requests'}})});
    await available.getByRole('button',{name:'Installer v1.0.0'}).click();
    await page.getByRole('status').filter({hasText:'installée et désactivée'}).waitFor();
    assert.deepEqual(sent,{extensionId:'media-requests',version:'1.0.0',upgrade:true,confirmDowngrade:true,expectedVersion:'1.0.3',expectedSha256:'digest'});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false); assert.deepEqual(errors,[]);
    await context.close();
  }} finally { await browser.close(); }
  console.log('Extension cards and version actions: 2/2');
}
run().catch(error=>{console.error(error);process.exitCode=1});

const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5197/app/';
const out = 'target/native-avatars-ui'; fs.mkdirSync(out, {recursive:true});
(async () => {
 const browser = await chromium.launch({channel:'chrome',headless:true});
 try { for (const width of [390,1440]) {
  const a = await setup(browser,width,'dark'); let migrated = false;
  await a.page.route('**/api/v1/admin/avatars/migrate-legacy', route => { if(route.request().method()==='POST') {migrated=true;return route.fulfill({json:{packs:1,images:1}})} return route.fulfill({json:migrated?{packs:0,images:0}:{packs:1,images:1}}); });
  await a.page.goto(base+'admin/libraries/#avatars-section');
  await a.page.getByRole('button',{name:'Transférer les collections'}).click();
  await a.page.getByText('1 collection(s) et 1 image(s) transférées.').waitFor(); assert(migrated);
  assert.equal(await a.page.getByText('Autoriser Gravatar').count(),0);
  assert.equal(await a.page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await a.page.screenshot({path:`${out}/admin-${width}.png`}); assert.deepEqual(a.errors,[]); await a.context.close();
  const s=await fixturePage(browser,width,'dark');let applied=false;
  await s.page.route('**/api/v1/account/avatar/collections**',route=>{let p=new URL(route.request().url()).pathname;if(p.endsWith('/one')){if(route.request().method()==='POST'){applied=true;return route.fulfill({status:204});}return route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="88" height="88"><rect width="88" height="88" fill="orange"/></svg>'});}return route.fulfill({json:p.endsWith('/pack')?{images:[{id:'one',name:'Témoin',url:'/api/v1/account/avatar/collections/pack/one'}]}:{packs:[{id:'pack',name:'Galerie',count:1}]}});});
  await s.page.goto(base+'settings/'); const open=s.page.getByRole('button',{name:'Choisir dans la galerie'});await open.click(); const dialog=s.page.getByRole('dialog',{name:'Choisir une photo de profil'});await dialog.waitFor();await dialog.getByRole('button',{name:'Témoin'}).click();await dialog.getByRole('button',{name:'Enregistrer',exact:true}).click();await s.page.getByText('Photo de profil enregistrée.').waitFor();assert(applied);await open.click();await dialog.waitFor();await s.page.keyboard.press('Escape');await s.page.waitForFunction(()=>!document.querySelector('dialog.avatar-modal')?.open);assert(await open.evaluate(el=>el===document.activeElement));assert.deepEqual(s.errors,[]); await s.page.screenshot({path:`${out}/profile-${width}.png`});await s.context.close(); console.log('PASS',width);
 } } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});

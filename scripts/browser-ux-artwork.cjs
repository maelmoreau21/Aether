const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const out = 'target/ux-artwork'; fs.mkdirSync(out, {recursive:true});
(async () => {
 const browser=await chromium.launch({channel:'chrome',headless:true});
 try {
  for (const width of [390,1440]) {
   const { page,context,errors }=await setup(browser,width,'dark');
   let calls=0;
   await page.route('**/api/v1/admin/artwork/**', async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.endsWith('/works')) return route.fulfill({json:url.searchParams.has('after') ? {items:[{workId:'b',title:'Deuxième film',posterCount:0}],nextCursor:null} : {items:[{workId:'a',title:'Premier film',posterCount:2}],nextCursor:'a'}});
    calls++; await new Promise(resolve=>setTimeout(resolve,500));
    return route.fulfill({json:calls===1 ? {items:[{workId:'a',title:'Premier film',result:{downloaded:2,errors:[],partial:false}}],nextCursor:'a'} : {items:[{workId:'b',title:'Deuxième film',error:{status:422,code:'external_id_required'}}],nextCursor:null}});
   });
   await page.goto('http://127.0.0.1:5183/app/admin/libraries/#artwork-section');
   await page.getByRole('heading',{name:'Images des médiathèques'}).waitFor();
   await page.locator('.artwork-admin select').first().selectOption('lib-film');
   await page.getByText('2 variantes enregistrées').waitFor();
   await page.getByRole('button',{name:'Page suivante'}).click(); await page.getByText('Deuxième film',{exact:true}).waitFor();
   await page.getByRole('button',{name:'Page précédente'}).click(); await page.getByText('Premier film',{exact:true}).waitFor();
   await page.getByRole('button',{name:'Lancer la collecte'}).click();
   await page.getByRole('button',{name:'Arrêter après cette œuvre'}).click();
   await page.getByRole('button',{name:'Reprendre la collecte'}).waitFor(); assert.equal(calls,1);
   await page.getByRole('button',{name:'Reprendre la collecte'}).click(); await page.getByText('Parcours terminé').waitFor();
   assert.equal(calls,2); await page.getByText('Œuvres à vérifier (1)').click(); await page.getByText('échec (external_id_required)').first().waitFor();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   assert.deepEqual(errors,[]); await page.screenshot({path:`${out}/artwork-${width}.png`,fullPage:true});
   console.log(`PASS ${width}: pagination, arrêt, reprise, erreur, affichage`); await context.close();
  }
 } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});


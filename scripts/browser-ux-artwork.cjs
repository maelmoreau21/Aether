const { chromium } = require('../apps/web/node_modules/playwright');
const { setup } = require('./browser-ux-admin.cjs');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/ux-artwork'; fs.mkdirSync(out, {recursive:true});
(async () => {
 const browser=await chromium.launch({channel:'chrome',headless:true});
 try {
  for (const width of [390,1440]) {
   const { page,context,errors }=await setup(browser,width,'dark');
   let calls=0, failList=false, lastQuery=null, job=null;
   let images=[{id:'one',kind:'poster',origin:'provider',url:'/ux-art/one',language:'fr'},{id:'two',kind:'poster',origin:'provider',url:'/ux-art/two',language:'fr'}];
   await page.route('**/api/v1/works/a/images', route=>route.fulfill({json:{images,originalLanguage:'ja'}}));
   await page.route('**/api/v1/works/a/images/similar?*', route=>route.fulfill({json:{pairs:[{first:'one',second:'two'}],uncompared:1,truncated:false}}));
   await page.route('**/api/v1/works/a/images/two', route=>{images=images.filter(item=>item.id!=='two');return route.fulfill({status:204});});
   await page.route('**/api/v1/admin/artwork/**', async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.endsWith('/works')) lastQuery=url.searchParams;
    if(url.pathname.endsWith('/works') && failList) return route.fulfill({status:503,json:{error:'synthetic_unavailable'}});
    if(url.pathname.endsWith('/works')) return route.fulfill({json:url.searchParams.has('after') ? {items:[{workId:'b',title:'Deuxième film',posterCount:0}],nextCursor:null} : {items:[{workId:'a',title:'Premier film',posterCount:2}],nextCursor:'a'}});
    if(url.pathname.endsWith('/jobs') && route.request().method()==='POST') {
     calls++; assert(route.request().headers()['x-csrf-token']); job={id:'durable',status:'queued',processed:0,downloaded:0,duplicates:0,skipped:0,errors:0,lastError:'',cursorWork:''}; return route.fulfill({status:202,json:{id:job.id}});
    }
    if(url.pathname.endsWith('/jobs')) return route.fulfill({json:job?[job]:[]});
    if(url.pathname.endsWith('/jobs/durable')) { job.status=route.request().postDataJSON().action==='pause'?'paused':route.request().postDataJSON().action==='resume'?'queued':'cancelled'; return route.fulfill({status:204}); }
    return route.fulfill({status:404,json:{error:'unexpected_fixture_request'}});
   });
   await page.goto((process.env.MEDIA_UI_URL || 'http://127.0.0.1:5192/app/')+'admin/libraries/#artwork-section');
   await page.getByRole('heading',{name:'Images des médiathèques'}).waitFor();
   await page.locator('.artwork-admin select').first().selectOption('lib-film');
   await page.getByText('2 variantes enregistrées').waitFor();
   await page.getByLabel('Rechercher un titre').fill('Premier'); await page.getByLabel('Variantes enregistrées').selectOption('with'); await page.getByRole('button',{name:'Rechercher',exact:true}).click(); await page.getByText('2 variantes enregistrées').waitFor(); assert.equal(lastQuery.get('q'),'Premier'); assert.equal(lastQuery.get('posters'),'with');
   await page.getByLabel('Rechercher un titre').fill(''); await page.getByLabel('Variantes enregistrées').selectOption('all'); await page.getByRole('button',{name:'Rechercher',exact:true}).click(); await page.getByText('2 variantes enregistrées').waitFor();
   await page.getByRole('button',{name:'Gérer les images'}).first().click(); const editor=page.getByRole('dialog',{name:'Modifier les images · Premier film'}); await editor.waitFor(); await editor.getByRole('button',{name:'Rechercher les images similaires'}).click(); await editor.getByText('1 paire trouvée.',{exact:false}).waitFor(); page.once('dialog',dialog=>dialog.accept()); await editor.getByRole('button',{name:'Retirer cette image'}).last().click(); await page.waitForFunction(()=>!document.querySelector('.similar-pair')); assert.equal(images.length,1); await page.getByRole('button',{name:'Fermer l’éditeur d’images'}).click();
   await page.getByRole('button',{name:'Page suivante'}).click(); await page.getByText('Deuxième film',{exact:true}).waitFor();
   await page.getByRole('button',{name:'Page précédente'}).click(); await page.getByText('Premier film',{exact:true}).waitFor();
   await page.getByRole('button',{name:'Lancer la collecte'}).click();
   await page.getByRole('button',{name:'Mettre en pause'}).click();
   await page.getByRole('button',{name:'Reprendre la collecte'}).waitFor(); assert.equal(calls,1);
   await page.reload(); await page.getByRole('heading',{name:'Images des médiathèques'}).waitFor(); await page.locator('.artwork-admin select').first().selectOption('lib-film');
   await page.getByRole('button',{name:'Reprendre la collecte'}).click(); await page.getByRole('button',{name:'Mettre en pause'}).waitFor(); assert.equal(calls,1);
   await page.getByRole('button',{name:'Annuler la collecte'}).click(); await page.getByRole('button',{name:'Lancer la collecte'}).waitFor();
   failList=true; await page.locator('.artwork-admin select').first().selectOption(''); await page.locator('.artwork-admin select').first().selectOption('lib-film'); await page.getByText('Liste des médias indisponible (HTTP 503).').waitFor(); failList=false; await page.getByRole('button',{name:'Réessayer'}).click(); await page.getByText('2 variantes enregistrées').waitFor();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   assert.deepEqual(errors,[]); await page.screenshot({path:`${out}/artwork-${width}.png`,fullPage:true});
   console.log(`PASS ${width}: pagination, collecte durable après reload, pause/reprise, erreur, affichage`); await context.close();
  }
 } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});


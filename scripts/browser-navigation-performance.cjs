// Chrome navigation timing on the real Svelte UI, with deliberately delayed API fixtures.
// Run against `npm run dev --prefix apps/web -- --port 5179`.
const {chromium}=require('../apps/web/node_modules/playwright');
const fs=require('node:fs'); const assert=require('node:assert/strict');
const {execFileSync}=require('node:child_process');
const files=['apps/web/src/lib/catalog/Catalog.svelte','apps/web/src/lib/catalog/api.ts','apps/web/src/lib/player/ImmersivePlayer.svelte'];
const saved=files.map(f=>fs.readFileSync(f));
const base='http://127.0.0.1:5179';
const film={id:'film-a',kind:'film',title:'Performance Film',year:2026,description:'Synthetic',libraryId:'lib-film',synthetic:true,editions:[{id:'edition',label:'Test',versions:[{id:'v',label:'MP4',sources:[{id:'s',assetIds:['asset'],playbackMediaId:'media-a'}]}]}]};
const media={id:'media-a',title:film.title,synthetic:true,sources:[{id:'source',audioTrackId:'fr',language:'fr',url:'/test-video',mime:'video/mp4'}],subtitles:[]};
const series={...film,id:'series-a',kind:'series',title:'Performance Series',libraryId:'lib-series',editions:[],seasons:[{id:'season',number:1,title:'Season',episodes:[{...film,id:'episode-a',kind:'episode',number:1,title:'Performance Episode',editions:[{...film.editions[0],versions:[...film.editions[0].versions,{id:'v2',label:'Other',sources:[{id:'s2',assetIds:['asset2'],playbackMediaId:'media-b'}]}]}]}]}]};
const delay=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
 const report={browser:browser.version(),scope:'Real UI, mocked API 180 ms catalogue / 300 ms descriptor, local synthetic MP4. No Core/Node/NAS proof.',runs:[]};
 try {
 for(const mode of process.env.MEDIA_COMPARE_BASELINE === '1' ? ['before','after'] : ['after']) {
  if(process.env.MEDIA_COMPARE_BASELINE === '1') files.forEach((f,i)=>fs.writeFileSync(f,mode==='before'?execFileSync('git',['show',`HEAD:${f}`]):saved[i]));
  await delay(1500);
  const context=await browser.newContext(); const page=await context.newPage(); let catalogRequests=0, descriptorRequests=0;
  const errors=[]; page.on('pageerror',e=>errors.push(String(e)));
  page.on('response',response=>{if(response.status()>=400 && /\.(svg|png|jpe?g|webp)$/.test(new URL(response.url()).pathname))errors.push(`Image HTTP ${response.status()}: ${new URL(response.url()).pathname}`);});
  await page.route('**/api/v1/**',async route=>{
   const u=new URL(route.request().url()); let data={};
   if(u.pathname==='/api/v1/auth/me') data={user:{id:'perf-user',login:'perf',role:'user'},csrfToken:'fixture'};
   else if(u.pathname==='/api/v1/preferences') data={locale:'fr',audioLanguage:'fr',subtitles:'off'};
   else if(u.pathname==='/api/v1/libraries') data={libraries:[{id:'lib-film',name:'Films',kind:'films'},{id:'lib-series',name:'Séries',kind:'series'}]};
   else if(u.pathname==='/api/v1/catalog') {catalogRequests++; await delay(180); data={items:[u.searchParams.get('kind')==='series'?series:film],total:1,offset:0,limit:24,notice:''};}
   else if(u.pathname.startsWith('/api/v1/catalog/')) data=u.pathname.endsWith('series-a')?series:film;
   else if(u.pathname.startsWith('/api/v1/media/')) {descriptorRequests++; await delay(300);data={...media,id:u.pathname.split('/').at(-1)};}
   else if(u.pathname.includes('progress')) data={items:[],positionSeconds:0};
   else if(u.pathname.includes('lists')) data={lists:[]};
   else if(u.pathname.includes('feedback')) data={rating:null};
   await route.fulfill({json:data});
  });
  await page.route('**/test-video',r=>r.fulfill({path:'tests/fixtures/generated/dual-opus/dual-opus.mp4',contentType:'video/mp4'}));
  await page.goto(base+'/app/');await page.locator('.catalog-home').waitFor();
  const returns=[];
  for(let i=0;i<10;i++) {
   await page.locator('a[href*="work=film-a"]').first().click(); await page.locator('.film-detail-view').waitFor();
   const requests=catalogRequests; const t=performance.now();
   await page.locator('a.brand').click(); await page.locator('.catalog-home').waitFor();
   returns.push({ms:performance.now()-t,requests:catalogRequests-requests});
  }
  await page.locator('a[href*="work=film-a"]').first().click(); await page.locator('.film-detail-view').waitFor();
  await delay(500);
  await page.evaluate(()=>{
   window.__launches=[];
   document.addEventListener('click',event=>{
    if(!event.target.closest('.film-detail-view .hero-cta'))return;
    const entry={start:performance.now()};window.__launches.push(entry);
    const observer=new MutationObserver(()=>{
     if(!document.querySelector('dialog[open]'))return;
     entry.shellMs=performance.now()-entry.start;observer.disconnect();
     const video=document.querySelector('dialog video');
     video.requestVideoFrameCallback(()=>entry.firstFrameMs=performance.now()-entry.start);
     video.addEventListener('playing',()=>entry.playingMs=performance.now()-entry.start,{once:true});
    });observer.observe(document.body,{subtree:true,attributes:true,childList:true});
   },true);
  });
  const openings=[];
  for(let i=0;i<2;i++) {
   const t=performance.now(); const n=descriptorRequests;
   await page.locator('.film-detail-view button').filter({hasText:/Lancer le film/}).first().click();
   await page.locator('dialog[open]').waitFor(); const shellMs=performance.now()-t;
   await page.waitForFunction(()=>{const v=document.querySelector('dialog video');return v&&v.readyState>=2&&v.currentTime>0});
   openings.push({shellMs,playingMs:performance.now()-t,descriptorRequests:descriptorRequests-n,ui:await page.evaluate(()=>window.__launches.at(-1))});
   await page.locator('dialog[open]').dispatchEvent('cancel'); await page.locator('dialog[open]').waitFor({state:'hidden'});
  }
  const scenarios=[];
  if(mode==='after') {
   await page.route('**/test-video', route => route.abort('failed'), { times: 1 });
   await page.locator('.film-detail-view button').filter({hasText:/Lancer le film/}).first().click();
   await page.locator('.player-error').waitFor();
   await page.locator('.player-error button').filter({hasText:'Réessayer'}).click();
   await page.waitForFunction(()=>{const video=document.querySelector('dialog video');return video&&!video.paused&&video.currentTime>0;});
   await page.locator('dialog[open]').dispatchEvent('cancel');
   scenarios.push('failed-initial-video-retry-really-plays');
   await page.locator('a.brand').click(); await page.locator('.catalog-home').waitFor();
   await page.evaluate(()=>{window.scrollTo({top:200,behavior:"instant"});document.querySelector('a[href*="work=film-a"]').click();});
   await page.locator('.film-detail-view').waitFor();
   await page.locator('a.brand').click();await page.locator('.catalog-home').waitFor();
   await page.waitForFunction(()=>Math.abs(window.scrollY-200)<=2);
   scenarios.push('catalogue-scroll-restored');
   // Observe actual blocking loaders rather than inferring absence from request counts.
   await page.evaluate(()=>{
    window.__blockingLoads=0;
    window.__observer=new MutationObserver(()=>{if(document.querySelector('.loading-empty'))window.__blockingLoads++});
    window.__observer.observe(document.body,{childList:true,subtree:true});
   });
   await page.locator('a[href*="work=series-a"]').first().click();await page.locator('.series-detail-view').waitFor();
   await page.locator('.episode-actions button').first().click();await page.locator('dialog[open]').waitFor();
   await page.waitForFunction(()=>{const v=document.querySelector('dialog video');return v&&v.currentTime>0});
   await page.locator('dialog[open]').dispatchEvent('cancel');
   await page.locator('.episode-actions button').first().click();await page.locator('dialog[open]').waitFor();
   await page.locator('dialog[open]').dispatchEvent('cancel');await delay(600);
   assert.equal(await page.locator('dialog[open]').count(),0);
   scenarios.push('cancel-episode-before-descriptor-no-late-reopen');
   await page.locator('a.brand').click();await page.locator('.catalog-home').waitFor();
   scenarios.push('home-series-episode-home');
   // Cold library navigation: intent warms the same authorized page used on click.
   const libraryLink=page.locator('a[href*="films/?library="]').first();
   const prefetched=page.waitForResponse(r=>r.url().includes('/api/v1/catalog?')&&r.url().includes('library=lib-film'));
   await libraryLink.hover();await prefetched;await delay(50);
   const prefetchedRequests=catalogRequests, intentStart=performance.now();
   await libraryLink.click();await page.locator('.catalog-grid-view .media-card').first().waitFor();
   assert.equal(catalogRequests,prefetchedRequests);
   assert.equal(await page.locator('.loading-empty').count(),0);
   scenarios.push({name:'cold-library-prefetch-shared',clickToCardsMs:performance.now()-intentStart,requestsAfterClick:catalogRequests-prefetchedRequests});
   // Visit the library sections with real client navigation links.
   await page.locator('a[href*="films/?library="]').first().click();await page.locator('.catalog-grid-view').waitFor(); await page.waitForFunction(()=>!document.querySelector('.loading-empty')); await delay(250);
   await page.locator('a[href*="series/?library="]').first().click();await page.locator('.catalog-grid-view').waitFor(); await page.waitForFunction(()=>!document.querySelector('.loading-empty')); await delay(250);
   await page.evaluate(()=>window.__blockingLoads=0);
   const n=catalogRequests;
   await page.locator('a[href*="films/?library="]').first().click();await page.locator('.catalog-grid-view').waitFor(); await page.waitForFunction(()=>!document.querySelector('.loading-empty')); await delay(250);
   assert.equal(catalogRequests,n);assert.equal(await page.evaluate(()=>window.__blockingLoads),0);scenarios.push('films-series-films-no-blocking-loader');
  }
  assert.equal(errors.length,0,errors.join('\n'));
  if(mode==='after') {assert(returns.every(r=>r.requests===0));assert(openings.every(r=>r.descriptorRequests===1));}
  report.runs.push({mode,returns,openings,scenarios,heapBytes:await page.evaluate(()=>performance.memory?.usedJSHeapSize ?? null),errors});await context.close();
 }
 fs.mkdirSync('target/navigation-performance',{recursive:true});fs.writeFileSync('target/navigation-performance/chrome.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
 }finally{if(process.env.MEDIA_COMPARE_BASELINE === '1') files.forEach((f,i)=>fs.writeFileSync(f,saved[i]));await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});


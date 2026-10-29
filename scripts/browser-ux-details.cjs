// UX detail regression: production Svelte build, synthetic API/artwork contracts.
// Optional MEDIA_UI_MEDIA_FIXTURE points only to a legally generated test video.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const base = process.env.MEDIA_UI_URL || 'http://127.0.0.1:5182/app/';
const out = process.env.MEDIA_REPORT_DIR || 'target/ux-details';
fs.mkdirSync(out, {recursive:true});
const report = { scope:'Production UI; intercepted synthetic APIs and images. Playback bytes from legal testsrc2/sine fixture, not Core/Node HTTP proof.', scenarios:[], screenshots:[], playback:[] };
const edition = ids => [{id:'edition',label:'Édition de test',versions:ids.map(id=>({id,label:id,sources:[{id:'source-'+id,assetIds:[id],playbackMediaId:id}]}))}];
const film = {id:'film-0',kind:'film',title:'Les horizons',originalTitle:'Horizons',kind:'film',year:2026,synthetic:false,description:'Une exploration des paysages et des histoires qui nous relient.',libraryId:'film',posterUrl:'/ux-art/poster',artwork:{backdrop:'/ux-art/backdrop',logo:'/ux-art/logo'},genres:['Documentaire','Voyage'],editions:edition(['version-mp4','version-mkv']),actors:[{name:'Artiste de test',role:'Narration'}]};
const episode = (season,number) => ({...film,id:`episode-${season}-${number}`,kind:'episode',number,title:`La lumière — épisode ${number}`,description:'Un résumé complet pour cet épisode. '.repeat(12),artwork:{thumb:'/ux-art/backdrop'},editions:edition([`episode-v-${season}-${number}`,`episode-alt-${season}-${number}`])});
const series = {...film,id:'series-0',kind:'series',title:'Les saisons',seasonCount:2,episodeCount:32,seasons:[{id:'season-1',number:1,title:'Saison 1',episodeCount:30,episodes:Array.from({length:30},(_,i)=>episode(1,i+1))},{id:'season-2',number:2,title:'Saison 2',episodeCount:2,episodes:[episode(2,1),episode(2,2)]}]};
const descriptor = id => ({id,workId:id.startsWith('episode')?'episode-1-1':'film-0',title:'Version de test',synthetic:true,available:true,sources:[{id:'source-'+id,audioTrackId:'internal-default',language:'fr',url:'/ux-stream/synthetic.mp4',mime:id==='version-mkv'?'video/x-matroska':'video/mp4',durationSeconds:34,tracks:[{index:0,codec_type:'video',codec_name:'av1',width:320,height:180,pix_fmt:'yuv420p10le'},{index:1,codec_type:'audio',codec_name:'opus',channels:1,tags:{language:'fra'}}]}],subtitles:[]});
async function setup(browser,width,theme,mode='normal',height=900) {
  const state = await fixturePage(browser,width,theme,mode==='broken'?'broken':'normal',height);
  const {page}=state;
  const requested=[];
  page.on('request',req=>{if(req.url().includes('/api/v1/media/'))requested.push(new URL(req.url()).pathname.split('/').pop());});
  await page.route('**/api/v1/feedback/**',route=>route.fulfill({json:{favorite:false,rating:null}}));
  await page.route('**/api/v1/progress/**',route=>route.fulfill({json:{workId:'film-0',position:0,duration:34,watched:false,revision:1}}));
  await page.route('**/api/v1/media/**',route=>route.fulfill({json:descriptor(new URL(route.request().url()).pathname.split('/').pop())}));
  await page.route('**/api/v1/catalog/**',async route=>{
    const url=new URL(route.request().url());
    let work=structuredClone(url.pathname.endsWith('series-0')?series:film);
    if(mode==='no-image') { work.artwork={};work.posterUrl=null; }
    if(mode==='long') { work.title='Un voyage à travers les paysages et les histoires de notre mémoire '.repeat(3);work.description='Un récit à découvrir en prenant le temps de lire. '.repeat(25); }
    if(mode==='offline')work.available=false;
    if(mode==='empty')work.seasons=[];
    if(mode==='single')work.editions=edition(['version-mp4']);
    if(mode==='paged' || mode==='episode-error') {
      const season=Number(url.searchParams.get('season')||1),offset=Number(url.searchParams.get('episodeOffset')||0);
      if(url.searchParams.has('season') && mode==='episode-error')return route.fulfill({status:503,json:{error:'test_unavailable'}});
      if(url.searchParams.has('season')) await new Promise(resolve=>setTimeout(resolve,250));
      work.episodesPage={season,offset,limit:24,nextOffset:offset+24,hasMore:season===1&&offset===0};
      work.seasons.forEach(item=>item.episodes=item.number===season?item.episodes.slice(offset,offset+24):[]);
    }
    return route.fulfill({json:work});
  });
  return {...state,requested};
}
async function shot(page,name,fullPage=false){await page.screenshot({path:path.join(out,name+'.png'),fullPage});report.screenshots.push(name);}
async function visibleBounds(page,label){
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,label+' document overflow');
  const clipped=await page.locator('.detail-info-column,.episode-content,.season-selector-label select').evaluateAll(nodes=>nodes.filter(node=>{const r=node.getBoundingClientRect();return r.left<0||r.right>innerWidth+1;}).map(node=>node.className));
  assert.deepEqual(clipped,[],label+' clipped controls');
}
async function run(){
  const browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
  report.browser=browser.version();
  try{
    for(const theme of ['light','dark'])for(const width of [320,360,390,430,768,1024,1440,1920]){
      const {page,context,errors}=await setup(browser,width,theme);
      await page.goto(base+'films/?work=film-0');await page.locator('.versions-panel').waitFor();
      await page.locator('.title-logo').evaluate(img=>img.decode());
      assert.equal(await page.locator('h1').count(),1);
      assert.equal(await page.locator('h1').innerText(),'');
      assert.equal(await page.locator('.versions-panel').getAttribute('open'),null);
      await visibleBounds(page,'film '+width);
      const primary=page.locator('.hero-cta');await primary.focus();
      assert.equal(await primary.evaluate(el=>getComputedStyle(el).outlineStyle),'solid');
      await shot(page,`${theme}-${width}-film`);
      await page.locator('.versions-panel summary').focus();await page.keyboard.press('Enter');
      await page.getByRole('button',{name:'Sélectionner la version MKV',exact:true}).click();
      assert.equal(await page.getByRole('button',{name:'Sélectionner la version MKV',exact:true}).getAttribute('aria-pressed'),'true');
      assert.ok((await page.locator('.specs-grid').innerText()).includes('OPUS'));
      await shot(page,`${theme}-${width}-versions`,true);
      await page.goto(base+'series/?work=series-0');await page.locator('.episode').first().waitFor();
      assert.equal(await page.locator('.season-shortcuts').count(),0);
      assert.equal(await page.getByRole('combobox',{name:'Saison',exact:true}).count(),1);
      assert.equal(await page.locator('.episode').count(),24);
      await visibleBounds(page,'series '+width);await shot(page,`${theme}-${width}-series`);
      const url=page.url();await page.getByRole('combobox',{name:'Saison',exact:true}).selectOption('2');
      assert.equal(page.url(),url);assert.equal(await page.locator('.episode').count(),2);
      await page.getByRole('combobox',{name:'Saison',exact:true}).selectOption('1');
      await page.getByRole('button',{name:'Suivants',exact:true}).click();
      assert.equal(await page.locator('.episode').count(),6);
      await page.getByRole('combobox',{name:'Saison',exact:true}).selectOption('2');
      assert.equal(await page.locator('.episode').count(),2,'Changing season resets page');
      await page.locator('.episode-versions summary').first().click();
      await page.locator('.episode-versions select').first().selectOption('episode-alt-2-1');
      await shot(page,`${theme}-${width}-episodes`,true);
      assert.deepEqual(errors,[]);
      report.scenarios.push({theme,width,film:true,singleSeasonSelector:true,versions:true,pagination:true,keyboard:true,overflow:false});
      await context.close();console.log('PASS',theme,width);
    }
    for(const mode of ['no-image','broken','long','offline','empty','paged','episode-error']){
      const {page,context,errors}=await setup(browser,320,'dark',mode);
      const isSeries=['empty','paged','episode-error'].includes(mode);
      await page.goto(base+(isSeries?'series/?work=series-0':'films/?work=film-0'));
      await page.locator(isSeries?'.series-detail-view':'.film-detail-view').waitFor();
      if(mode==='broken'){await page.waitForFunction(()=>document.querySelector('h1')?.textContent==='Les horizons');assert.equal(await page.locator('.detail-backdrop').count(),0);}
      if(mode==='no-image')assert.equal(await page.locator('.detail-backdrop').count(),0);
      if(mode==='long'){await page.locator('.long-synopsis summary').click();assert.ok((await page.locator('.long-synopsis p').innerText()).length>300);}
      if(mode==='offline')assert.equal(await page.locator('.hero-cta').isDisabled(),true);
      if(mode==='empty')await page.getByText('Aucun épisode disponible pour cette saison.').waitFor();
      if(mode==='paged'){await page.getByRole('button',{name:'Suivants',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.episode').length===6);await page.getByRole('combobox',{name:'Saison',exact:true}).selectOption('2');await page.waitForFunction(()=>document.querySelectorAll('.episode').length===2);}
      if(mode==='episode-error'){await page.getByRole('combobox',{name:'Saison',exact:true}).selectOption('2');await page.getByRole('button',{name:'Revenir à la première saison'}).waitFor();assert.equal(await page.locator('.episode').count(),0);await page.getByRole('button',{name:'Revenir à la première saison'}).click();await page.waitForFunction(()=>document.querySelectorAll('.episode').length===24);}
      await visibleBounds(page,mode);await shot(page,'state-'+mode,true);assert.deepEqual(errors,[]);report.scenarios.push({mode,pass:true});await context.close();
    }
    const fixture=process.env.MEDIA_UI_MEDIA_FIXTURE||'target/streaming-video-window-poc/gop15.mp4';
    if(fs.existsSync(fixture)){
      const bytes=fs.readFileSync(fixture);assert.ok(bytes.length<10*1024*1024,'Use a small synthetic fixture');
      for(const kind of ['film','episode']){
        const {page,context,errors,requested}=await setup(browser,390,'dark','single');
        await page.route('**/ux-stream/synthetic.mp4',route=>{
          const range=route.request().headers().range;const match=/bytes=(\d+)-(\d*)/.exec(range||'');
          const start=match?Number(match[1]):0,end=match&&match[2]?Math.min(Number(match[2]),bytes.length-1):bytes.length-1;
          if(start>=bytes.length)return route.fulfill({status:416,headers:{'content-range':`bytes */${bytes.length}`}});
          return route.fulfill({status:match?206:200,headers:{'content-type':'video/mp4','accept-ranges':'bytes',...(match?{'content-range':`bytes ${start}-${end}/${bytes.length}`}:{})},body:bytes.subarray(start,end+1)});
        });
        await page.goto(base+(kind==='film'?'films/?work=film-0':'series/?work=series-0'));
        await page.locator(kind==='film'?'.hero-cta':'.episode-play').first().click();
        await page.getByRole('dialog',{name:'Lecteur immersif'}).waitFor();
        await page.waitForFunction(()=>{const v=document.querySelector('#player video');return v&&v.currentTime>1&&v.videoWidth>0;},{},{timeout:20000});
        report.playback.push({kind,fixture:path.resolve(fixture),...await page.locator('#player video').evaluate(v=>({time:v.currentTime,width:v.videoWidth,height:v.videoHeight,frames:v.getVideoPlaybackQuality().totalVideoFrames}))});
        await shot(page,kind+'-playback');
        await page.locator('.player-shell').focus();
        await page.keyboard.press('Space');
        await page.waitForFunction(()=>document.querySelector('#player video')?.paused);
        await page.keyboard.press('ArrowRight');
        await page.waitForFunction(()=>document.querySelector('#player video')?.currentTime>=5);
        await page.keyboard.press('Space');
        await page.waitForFunction(()=>!document.querySelector('#player video')?.paused);
        await page.keyboard.press('Escape');
        await page.waitForFunction(()=>!document.fullscreenElement);
        if(await page.locator('#player').evaluate(dialog=>dialog.open))await page.keyboard.press('Escape');
        await page.waitForFunction(()=>!document.querySelector('#player')?.open);
        assert.ok(requested.some(id=>id.startsWith(kind==='film'?'version-mp4':'episode-v-1-1')));
        assert.deepEqual(errors,[]);await context.close();
      }
    }else report.playback.push({skipped:'Legal test fixture not available',fixture});
    const {page,context}=await setup(browser,844,'dark','normal',390);await page.goto(base+'series/?work=series-0');await page.locator('.episode').first().waitFor();await visibleBounds(page,'landscape');const beforeJump=page.url();await page.locator('.hero-cta').tap();
    await page.waitForFunction(()=>{const box=document.getElementById('episodes-shelf-title')?.getBoundingClientRect();return box&&box.top>=64&&box.bottom<innerHeight-64;});
    assert.equal(page.url(),beforeJump,'Jumping to episodes must not reload the catalogue route');
    assert.equal(await page.evaluate(()=>document.activeElement.id),'episodes-shelf-title');
    await shot(page,'landscape-episodes');await context.close();
  }finally{fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));await browser.close();}
  console.log('Completed',report.scenarios.length,'detail scenarios;',report.playback.length,'playback checks');
}
if(require.main===module)run().catch(error=>{console.error(error);process.exitCode=1;});
module.exports={setup};

const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const restart = process.env.MEDIA_TEST_RESTART === '1';
const out = process.env.MEDIA_REPORT_DIR || 'docs/evidence/2026-10-18/external-tracks';
if (!restart) fs.rmSync(out, {recursive: true, force: true});
fs.mkdirSync(out, {recursive: true});
(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  const report = {browser:browser.version(), os:require('node:os').release(), headless:true, restart, results:{}};
  try {
    const context = await browser.newContext({viewport:{width:1440,height:900}});
    const page = await context.newPage();
    const errors=[];
    page.on('response', async response=>{if(response.url().endsWith('/scan')) console.log('scan HTTP',response.status(),await response.text());});
    page.on('pageerror', error=>errors.push(String(error)));
    page.on('console', message=>{if(message.type()==='error' && /Content Security Policy/i.test(message.text())) errors.push(message.text());});
    await page.goto(origin+'/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('scanner-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable scanner browser password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin+'/app/');
    if (!restart) {
      await page.goto(origin+'/app/admin/libraries/');
      await page.getByLabel('Nom de la bibliothèque',{exact:true}).fill('Films scanner navigateur');
      await page.getByRole('button',{name:'Enregistrer',exact:true}).click();
      await page.getByRole('heading',{name:'Films scanner navigateur',exact:true}).waitFor();
      await page.getByRole('button',{name:'Scanner Films scanner navigateur',exact:true}).click();
      const article = page.locator('article').filter({has:page.getByRole('heading',{name:'Films scanner navigateur',exact:true})});
      await article.getByText(/Scan : Terminé/).waitFor({timeout:60000});
      assert.match(await article.locator('.scan-counters').textContent(), /2 découverts/);
      report.results.scan = true;
      await page.screenshot({path:`${out}/scan.png`});
      await page.getByRole('button',{name:'Scanner Films scanner navigateur',exact:true}).click();
      await article.locator('.scan-counters').filter({hasText:/0 analysés.*2 réutilisés/}).waitFor({timeout:60000});
      await article.getByText(/Scan : Terminé/).waitFor({timeout:60000});
      report.results.incremental = true;
    }
    await page.goto(origin+'/app/films/');
    const card=page.locator('.media-card').filter({hasText:'Scanner Film'});
    await card.waitFor();
    assert.equal(await page.locator('.media-card').count(), 1);
    await card.click();
    await page.getByRole('heading',{name:'Scanner Film',exact:true}).waitFor();
    const container=restart?'MKV':'MP4';
    await page.getByRole('button',{name:`Sélectionner la version ${container}`,exact:true}).click();
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>document.querySelector('video')?.currentTime>0.2);
    await page.waitForFunction(()=>{ const v=document.querySelector('video'); return [...v.textTracks].some(t=>t.mode==='showing' && t.activeCues?.length && t.activeCues[0].text.includes('Sous-titre externe')); });
    await page.locator('[data-menu="audio"]').click();
    await page.locator('[data-audio-id="external-0"]').click();
    await page.waitForFunction(()=>{const a=document.querySelector('audio[data-external-audio]'); const v=document.querySelector('video'); return a && !a.paused && a.currentTime > .2 && v.muted && Math.abs(a.currentTime-v.currentTime)<.3;});
    // A decoded audio signal is observed through WebAudio, separately from metadata.
    const pcm = await page.evaluate(async()=>{
      const audio=document.querySelector('audio[data-external-audio]');
      const context=new AudioContext(); await context.resume(); const source=context.createMediaElementSource(audio);
      const analyser=context.createAnalyser(); source.connect(analyser); analyser.connect(context.destination);
      const samples=new Float32Array(2048); let peak=0;
      for(let i=0;i<20;i++){analyser.getFloatTimeDomainData(samples); peak=Math.max(peak,...samples.map(Math.abs)); await new Promise(r=>setTimeout(r,20));}
      source.disconnect(); await context.close(); return peak;
    });
    assert.ok(pcm>.001, `external audio PCM peak ${pcm}`);
    await page.evaluate(()=>{const video=document.querySelector('video');video.pause();video.currentTime=4;});
    await page.waitForFunction(()=>{const video=document.querySelector('video');return !video.seeking && video.paused && Math.abs(video.currentTime-4)<0.3;});
    await page.waitForFunction(()=>{const a=document.querySelector('audio[data-external-audio]');return a && a.paused && Math.abs(a.currentTime-4)<.3;});
    report.captionLayout=await page.evaluate(()=>{const cue=[...document.querySelector('video').textTracks].find(t=>t.mode==='showing')?.activeCues?.[0];return cue?{line:cue.line,snapToLines:cue.snapToLines}:null;});
    await page.screenshot({path:`${out}/subtitle-rendered${restart?'-restart':''}.png`});
    await page.locator('[data-menu="subtitles"]').click();
    await page.locator('[data-subtitle-id]').filter({hasText:'EN · VTT'}).click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].some(t=>t.mode==='showing' && t.activeCues?.[0]?.text==='External WebVTT subtitle'));
    await page.screenshot({path:`${out}/webvtt-rendered${restart?'-restart':''}.png`});
    await page.locator('[data-menu="subtitles"]').click(); await page.locator('[data-subtitle-id="off"]').click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].every(t=>t.mode!=='showing'));
    report.results.externalTracks={container,srtRendered:true,webvttRendered:true,externalOpusPcm:pcm,audioVideoSync:true,pausedSeek:true,subtitlesOff:true};
    await page.screenshot({path:`${out}/playback${restart?'-restart':''}.png`});
    await page.locator('button.return').click();
    await page.waitForFunction(async()=>{ const r=await fetch('/api/v1/progress'); const data=await r.json(); return data.items.some(item=>!item.watched && Math.abs(item.position-4)<.5); });
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>{const v=document.querySelector('video');return !v.paused && v.currentTime>3.8 && v.currentTime<7;});
    await page.locator('button.return').click();
    await page.getByRole('button',{name:'Marquer comme vu',exact:true}).click();
    await page.getByRole('button',{name:'Marquer non vu',exact:true}).waitFor();
    await page.getByRole('button',{name:'Marquer non vu',exact:true}).click();
    await page.getByRole('button',{name:'Marquer comme vu',exact:true}).waitFor();
    report.results.progress={saved:true,resumed:true,manualWatched:true,manualUnwatched:true};
    assert.deepEqual(errors, []);
    report.results = {...report.results,catalogue:true,singleWorkTwoVersions:true,playback:true,seek:true,pause:true,cspErrors:errors};
    report.passed=true;
    await context.close();
  } catch(error) {report.failure=String(error.stack);const p=browser.contexts()[0]?.pages()[0];if(p) report.diagnostic=await p.evaluate(()=>({error:document.querySelector('.player-error')?.textContent,notice:document.querySelector('.fullscreen-notice')?.textContent,video:{time:document.querySelector('video')?.currentTime,paused:document.querySelector('video')?.paused},audio:{time:document.querySelector('audio')?.currentTime,paused:document.querySelector('audio')?.paused}})).catch(()=>null);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report${restart?'-restart':''}.json`, JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

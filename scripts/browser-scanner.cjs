const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
if (!origin || !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin)) throw new Error('isolated test origin required');
const restart = process.env.MEDIA_TEST_RESTART === '1';
const long = process.env.MEDIA_TEST_LONG === '1';
const out = process.env.MEDIA_REPORT_DIR || (long ? 'docs/evidence/2026-10-18/player-long' : 'docs/evidence/2026-10-16/scanner');
// Keep previous evidence; each successful run replaces its own reports only.
fs.mkdirSync(out, {recursive: true});
(async () => {
  const browser = await chromium.launch({channel:'chrome', headless:true});
  const report = {browser:browser.version(), os:require('node:os').release(), headless:true, restart, results:{}};
  try {
    const context = await browser.newContext({viewport:{width:1440,height:900}, locale:'fr-FR'});
    const page = await context.newPage();
    page.setDefaultTimeout(30000);
    async function openMenu(name) { await page.keyboard.press('Shift'); await page.locator(`[data-menu="${name}"]`).click(); }
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
      await page.getByRole('button',{name:'Ajouter une médiathèque',exact:true}).click();
      await page.getByLabel('Nom de la bibliothèque',{exact:true}).fill('Films scanner navigateur');
      await page.getByRole('dialog').getByRole('button',{name:'Enregistrer',exact:true}).click();
      await page.getByRole('heading',{name:'Films scanner navigateur',exact:true}).waitFor();
      await page.getByLabel('Actions Films scanner navigateur',{exact:true}).click();
      await page.getByRole('button',{name:'Actualiser la médiathèque',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
      const article = page.locator('article').filter({has:page.getByRole('heading',{name:'Films scanner navigateur',exact:true})});
      await article.getByText(/Terminé/).waitFor({timeout:60000});
      assert.match(await article.locator('.scan-counters').textContent(), /2 (?:fichier\(s\) )?découvert/);
      report.results.scan = true;
      await page.screenshot({path:`${out}/scan.png`});
      await page.getByLabel('Actions Films scanner navigateur',{exact:true}).click();
      await page.getByRole('button',{name:'Actualiser la médiathèque',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
      await article.locator('.scan-counters').filter({hasText:/0 analysé.*2 réutilisé/}).waitFor({timeout:60000});
      await article.getByText(/Terminé/).waitFor({timeout:60000});
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
    // Internal multi-track Opus: decode distinguishable synthetic FR/EN signals.
    report.results.internalSwitches = [];
    let preparedVideoUrl;
    for (const id of ['internal-1','internal-2','internal-1','internal-2']) {
      await openMenu('audio');
      await page.locator(`[data-audio-id="${id}"]`).click();
      await page.waitForFunction(id=>{const a=document.querySelector('audio[data-external-audio]'); const v=document.querySelector('video'); return a && a.currentSrc.endsWith('/prepared/audio-'+id.split('-')[1]) && !a.paused && !v.paused && v.currentTime>.2 && Math.abs(a.currentTime-v.currentTime)<.3;},id,{timeout:30000});
      const sample = await page.evaluate(async()=>{
        const v=document.querySelector('video'), a=document.querySelector('audio[data-external-audio]');
        const c=new AudioContext(); await c.resume(); const s=c.createMediaElementSource(a), n=c.createAnalyser(); n.fftSize=8192;s.connect(n);n.connect(c.destination);
        await new Promise(r=>setTimeout(r,250));const bins=new Float32Array(n.frequencyBinCount);n.getFloatFrequencyData(bins);
        let peak=0;for(let i=1;i<bins.length;i++)if(bins[i]>bins[peak])peak=i;
        const result={frequency:peak*c.sampleRate/n.fftSize,db:bins[peak],url:v.currentSrc,time:v.currentTime,fullscreen:!!document.fullscreenElement};s.disconnect();await c.close();return result;
      });
      assert.ok(sample.db>-80,`decoded PCM ${JSON.stringify(sample)}`);
      assert.ok(id==='internal-2' ? sample.frequency>950 && sample.frequency<1150 : sample.frequency>400 && sample.frequency<900, `language signal ${JSON.stringify(sample)}`);
      if(preparedVideoUrl) assert.equal(sample.url,preparedVideoUrl); preparedVideoUrl=sample.url;
      assert.equal(sample.fullscreen,true);
      report.results.internalSwitches.push({id,...sample});
    }
    if (long) {
      const cdp = await context.newCDPSession(page); await cdp.send('Performance.enable');
      const before = await cdp.send('Performance.getMetrics');
      report.results.long = {duration:await page.locator('video').evaluate(v=>v.duration),seeks:[]};
      assert.ok(report.results.long.duration>5390);
      for(const [step,target] of [1,2700,5380].entries()) {
        const switchId=step===1 ? 'internal-2' : 'internal-1';
        await page.locator('video').evaluate((v,t)=>{v.pause();v.currentTime=t;},target);
        await page.waitForFunction(t=>{const v=document.querySelector('video'),a=document.querySelector('audio');return v.paused && !v.seeking && Math.abs(v.currentTime-t)<.3 && a && a.paused && Math.abs(a.currentTime-t)<.3;},target);
        const beforeSwitch=await page.locator('video').evaluate(v=>({time:v.currentTime,url:v.currentSrc}));
        await openMenu('audio');await page.locator(`[data-audio-id="${switchId}"]`).click();
        await page.waitForFunction(({t,id})=>{const v=document.querySelector('video'),a=document.querySelector('audio');return v.paused && a && a.currentSrc.endsWith('/prepared/audio-'+id.split('-')[1]) && a.paused && Math.abs(v.currentTime-t)<.3 && Math.abs(a.currentTime-t)<.3;},{t:target,id:switchId});
        assert.equal(await page.locator('video').evaluate(v=>v.currentSrc),beforeSwitch.url);
        await page.locator('video').evaluate(v=>v.play());
        await page.waitForFunction(t=>document.querySelector('video').currentTime>t+.3,target);
        report.results.long.seeks.push({target,passed:true});
      }
      await page.locator('video').evaluate(v=>{v.pause();v.currentTime=2700;});
      await page.waitForFunction(()=>!document.querySelector('video').seeking);
      await page.locator('video').evaluate(v=>v.play());
      const samples=[];
      for(let i=0;i<12;i++) { await page.waitForTimeout(5000); samples.push(await page.evaluate(()=>{const v=document.querySelector('video'),a=document.querySelector('audio');return {time:v.currentTime,drift:Math.abs(v.currentTime-a.currentTime),paused:v.paused};})); }
      assert.ok(samples.at(-1).time-samples[0].time>50);assert.ok(samples.every(s=>!s.paused && s.drift<.3));
      report.results.long.playbackSamples=samples;
      const after=await cdp.send('Performance.getMetrics');
      report.results.long.jsHeap={before:before.metrics.find(m=>m.name==='JSHeapUsedSize')?.value,after:after.metrics.find(m=>m.name==='JSHeapUsedSize')?.value};
      const interruptedAt=await page.locator('video').evaluate(v=>v.currentTime);
      await page.route('**/prepared/audio-*',route=>route.abort('internetdisconnected'));
      await openMenu('audio');await page.locator('[data-audio-id="internal-2"]').click();
      await page.locator('.player-error').waitFor();
      await page.unroute('**/prepared/audio-*');
      await page.getByRole('button',{name:'Réessayer explicitement',exact:true}).click();
      await page.locator('.loading').waitFor({state:'detached'});await page.locator('.player-error').waitFor({state:'detached'});
      assert.ok(Math.abs(await page.locator('video').evaluate(v=>v.currentTime)-interruptedAt)<1);
      report.results.long.interruptedAudioRequestRecovered=true;
      await page.locator('video').evaluate(v=>{v.pause();v.currentTime=2;});await page.waitForFunction(()=>!document.querySelector('video').seeking);await page.locator('video').evaluate(v=>v.play());
    }
    await openMenu('subtitles');
    await page.locator('[data-subtitle-id]').filter({hasText:'EN · ASS'}).click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].some(t=>t.mode==='showing' && t.activeCues?.[0]?.text.includes('ASS Hello, world')));
    report.results.assTextFallback=true;
    await openMenu('subtitles');
    await page.getByLabel('Décalage sous-titres en secondes',{exact:true}).fill('1');
    await page.locator('.menu-heading').click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].find(t=>t.mode==='showing')?.cues?.[0]?.startTime===1);
    await page.getByLabel('Décalage sous-titres en secondes',{exact:true}).fill('0');
    await page.locator('.menu-heading').click();
    await page.locator('[aria-label="Fermer le menu"]').click();
    await page.screenshot({path:`${out}/ass-fallback${restart?'-restart':''}.png`});
    if (!long) {
      await page.locator('video').evaluate(v=>{v.pause();v.currentTime=4.25;});
      await page.waitForFunction(()=>!document.querySelector('video').seeking);
      await openMenu('subtitles'); await page.locator('[data-subtitle-id="internal-4"]').click();
      await page.waitForFunction(()=>[...document.querySelector('video').textTracks].some(t=>t.mode==='showing' && [...(t.activeCues||[])].some(c=>c.text.includes('FR FORCE'))));
      await openMenu('subtitles');
      await page.keyboard.press('End');await page.keyboard.press('Enter');
      assert.equal(await page.getByLabel('Décalage sous-titres en secondes',{exact:true}).evaluate(e=>document.activeElement===e),true);
      await page.getByLabel('Décalage sous-titres en secondes',{exact:true}).fill('-.5');await page.locator('.menu-heading').click();
      await page.waitForFunction(()=>Math.abs([...document.querySelector('video').textTracks].find(t=>t.mode==='showing').cues[0].startTime-1.5)<.05);
      await page.getByLabel('Décalage sous-titres en secondes',{exact:true}).fill('0');await page.locator('.menu-heading').click();
      await page.locator('[aria-label="Fermer le menu"]').click();
      await page.screenshot({path:`${out}/integrated-forced${restart?'-restart':''}.png`});
      report.results.integratedSubtitleAndNegativeOffset=true;
      await page.locator('video').evaluate(v=>v.play());
    }
    await openMenu('audio');
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
    await openMenu('audio');
    for(const offset of [.25,-.25,0]) {
      await page.getByLabel('Décalage audio en secondes',{exact:true}).fill(String(offset));
      await page.locator('.menu-heading').click();
      await page.waitForFunction(o=>Math.abs(document.querySelector('audio[data-external-audio]').currentTime-(4-o))<.15,offset);
    }
    await page.locator('[aria-label="Fermer le menu"]').click();
    report.results.signedOffsets=true;
    report.captionLayout=await page.evaluate(()=>{const cue=[...document.querySelector('video').textTracks].find(t=>t.mode==='showing')?.activeCues?.[0];return cue?{line:cue.line,snapToLines:cue.snapToLines}:null;});
    await page.screenshot({path:`${out}/subtitle-rendered${restart?'-restart':''}.png`});
    await openMenu('subtitles');
    await page.locator('[data-subtitle-id]').filter({hasText:'EN · VTT'}).click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].some(t=>t.mode==='showing' && t.activeCues?.[0]?.text==='External WebVTT subtitle'));
    await page.screenshot({path:`${out}/webvtt-rendered${restart?'-restart':''}.png`});
    await openMenu('subtitles'); await page.locator('[data-subtitle-id="off"]').click();
    await page.waitForFunction(()=>[...document.querySelector('video').textTracks].every(t=>t.mode!=='showing'));
    report.results.externalTracks={container,srtRendered:true,webvttRendered:true,externalOpusPcm:pcm,audioVideoSync:true,pausedSeek:true,subtitlesOff:true};
    await page.screenshot({path:`${out}/playback${restart?'-restart':''}.png`});
    await page.keyboard.press('Shift'); await page.locator('button.return').click();
    await page.waitForFunction(async()=>{ const r=await fetch('/api/v1/progress'); const data=await r.json(); return data.items.some(item=>!item.watched && Math.abs(item.position-4)<.5); });
    await page.locator('.hero-cta').click();
    await page.waitForFunction(()=>{const v=document.querySelector('video');return !v.paused && v.currentTime>3.8 && v.currentTime<7;});
    await page.keyboard.press('Shift'); await page.locator('button.return').click();
    await page.waitForFunction(()=>!document.querySelector('dialog#player')?.hasAttribute('open'));
    await page.waitForFunction(async()=>{ const r=await fetch('/api/v1/progress'); const data=await r.json(); return data.items.some(item=>item.position>3.8); });
    // Seeking near the end of a long media can legitimately set watched automatically.
    if (long && await page.getByRole('button',{name:'Marquer non vu',exact:true}).isVisible()) await page.getByRole('button',{name:'Marquer non vu',exact:true}).click();
    await page.getByRole('button',{name:'Marquer comme vu',exact:true}).click();
    await page.getByRole('button',{name:'Marquer non vu',exact:true}).waitFor();
    await page.getByRole('button',{name:'Marquer non vu',exact:true}).click();
    await page.getByRole('button',{name:'Marquer comme vu',exact:true}).waitFor();
    report.results.progress={saved:true,resumed:true,manualWatched:true,manualUnwatched:true};
    assert.deepEqual(errors, []);
    report.results = {...report.results,catalogue:true,singleWorkTwoVersions:true,playback:true,seek:true,pause:true,cspErrors:errors};
    report.passed=true;
    await context.close();
  } catch(error) {report.failure=String(error.stack);const p=browser.contexts()[0]?.pages()[0];if(p) report.diagnostic=await p.evaluate(()=>({url:location.href,buttons:[...document.querySelectorAll('button')].map(b=>b.textContent).slice(-30),error:document.querySelector('.player-error')?.textContent,notice:document.querySelector('.fullscreen-notice')?.textContent,video:{time:document.querySelector('video')?.currentTime,paused:document.querySelector('video')?.paused},audio:{time:document.querySelector('audio')?.currentTime,paused:document.querySelector('audio')?.paused}})).catch(()=>null);process.exitCode=1;}
  finally {await browser.close();fs.writeFileSync(`${out}/report${restart?'-restart':''}.json`, JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

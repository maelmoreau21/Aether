const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const origin = process.env.MEDIA_TEST_ORIGIN;
assert.match(origin ?? '', /^http:\/\/127\.0\.0\.1:\d+$/);
const restart = process.env.MEDIA_TEST_RESTART === '1';
const out = process.env.MEDIA_REPORT_DIR || 'output/playwright/statistics';
fs.mkdirSync(out, {recursive:true});
(async()=>{
  const browser = await chromium.launch({channel:process.env.MEDIA_BROWSER_CHANNEL || 'chrome',headless:true});
  const context = await browser.newContext({viewport:{width:1440,height:1000},locale:'fr-FR'});
  const page = await context.newPage();
  page.setDefaultTimeout(30000);
  const report = {browser:browser.version(),restart,scope:'Synthetic local AV1/Opus and Opus fixtures; no NAS, provider or physical-device proof'};
  const errors=[];
  page.on('pageerror',error=>errors.push(String(error)));
  const read = path => page.evaluate(async path=>{
    const response=await fetch(path,{cache:'no-store'});if(!response.ok)throw new Error(`HTTP ${response.status} ${path}`);return response.json();
  },path);
  const waitRead = async (path, predicate) => {
    const deadline=Date.now()+30000;
    do { const data=await read(path);if(predicate(data))return data;await new Promise(resolve=>setTimeout(resolve,150)); }while(Date.now()<deadline);
    throw new Error(`Timed out waiting for ${path}`);
  };
  try {
    await page.goto(`${origin}/app/`);await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill('statistics-admin');
    await page.locator('input[autocomplete="current-password"]').fill('disposable statistics browser password');
    await page.getByRole('button',{name:'Se connecter',exact:true}).click();await page.waitForURL(`${origin}/app/`);
    await page.goto(`${origin}/app/admin/libraries/#statistics-section`);
    await page.getByRole('heading',{name:'Statistiques',exact:true}).waitFor();
    await page.getByText('Collecte et conservation',{exact:true}).click();
    const toggle=page.getByLabel('Activer les statistiques détaillées');
    if(restart){
      assert.equal((await read('/api/v1/admin/statistics')).enabled,false);
      const persisted=await read('/api/v1/admin/statistics');assert(persisted.summary.seconds>0);assert(persisted.summary.plays>=3);
      await page.screenshot({path:`${out}/persisted.png`,fullPage:true});
      report.persisted=true;
    }else{
      assert.equal((await read('/api/v1/playback/statistics')).enabled,false);
      await toggle.check();await page.getByRole('button',{name:'Enregistrer',exact:true}).click();
      await page.getByText('Paramètres enregistrés.',{exact:true}).waitFor();
      assert.equal((await read('/api/v1/playback/statistics')).enabled,true);
      await page.goto(`${origin}/app/films/`);
      await page.locator('.media-card').filter({hasText:'Statistics Film'}).first().click({position:{x:12,y:12}});
      await page.getByRole('heading',{name:'Statistics Film',exact:true}).waitFor();
      await page.getByRole('button',{name:'Sélectionner la version MP4',exact:true}).click();
      await page.locator('.hero-cta').click();
      await page.waitForFunction(()=>document.querySelector('video')?.currentTime>1);
      await page.keyboard.press('Shift');await page.getByRole('button',{name:'Pause',exact:true}).click();
      await page.getByRole('button',{name:'Lire',exact:true}).click();
      await page.locator('[data-menu="audio"]').click();
      await page.locator('[data-audio-id="internal-2"]').click();
      await waitRead('/api/v1/preferences',data=>data.audioTrack?.trackId==='internal-2');
      await page.waitForFunction(()=>{const v=document.querySelector('video'),a=document.querySelector('audio[data-active-audio="true"]');return v&&!v.paused&&v.currentTime>1&&a&&!a.paused;});
      // Freeze the twelve-second fixture while verifying API delivery and rejected choices.
      await page.keyboard.press('Shift');await page.getByRole('button',{name:'Pause',exact:true}).click();
      const accepted=await waitRead('/api/v1/admin/statistics',data=>data.behavior.audioChanges>=1);
      const rejectedAudio=/\/prepared\/(?:audio-1|opus-2)/;
      await page.route(rejectedAudio,route=>route.abort('failed'));
      await page.locator('[data-menu="audio"]').click();await page.locator('[data-audio-id="internal-1"]').click();
      await page.locator('.fullscreen-notice').filter({hasText:/piste précédente.*conservée/i}).waitFor();
      assert.equal((await read('/api/v1/preferences')).audioTrack.trackId,'internal-2');
      assert.equal((await read('/api/v1/admin/statistics')).behavior.audioChanges,accepted.behavior.audioChanges);
      await page.unroute(rejectedAudio);report.failedAudioNotCounted=true;
      await page.locator('[data-menu="subtitles"]').click();await page.locator('[data-subtitle-id="off"]').click();
      // English audio: the choice is kept in the original-version (VO) subtitle memory.
      await waitRead('/api/v1/preferences',data=>data.subtitlePreference?.enabled===false||data.subtitlePreferenceForeign?.enabled===false);
      await waitRead('/api/v1/admin/statistics',data=>data.behavior.subtitleChanges>=1);report.subtitles=true;
      await page.keyboard.press('Shift');await page.locator('button.return').click();
      const video=await waitRead('/api/v1/admin/statistics',data=>data.behavior.audioChanges>=1);
      assert(video.summary.seconds>0);assert(video.behavior.audioChanges>=1);
      const videoDetail=await read(`/api/v1/admin/statistics/sessions/${video.sessions[0].id}`);
      assert(videoDetail.events.some(row=>row.event.kind==='audio_change'&&row.event.origin==='manual'&&row.event.track?.id==='internal-2'));
      report.video=true;
      const mkvChoice=page.getByRole('button',{name:'S\u00e9lectionner la version MKV',exact:true});
      await mkvChoice.click();await page.locator('.hero-cta').click();
      await page.waitForFunction(()=>{const v=document.querySelector('video'),a=document.querySelector('audio[data-active-audio="true"]');return v&&!v.paused&&v.currentTime>1&&a&&!a.paused&&a.dataset.internalAudioTrack==='internal-2';});
      await page.keyboard.press('Shift');await page.locator('button.return').click();report.mkv=true;
      await page.goto(`${origin}/app/music/`);
      await page.getByRole('button',{name:'Lire Statistics Song',exact:true}).click();
      await page.waitForFunction(()=>[...document.querySelectorAll('audio')].some(a=>a.currentSrc.includes('/music/tracks/')&&!a.paused&&a.currentTime>1));
      // Reaching the end by seek must not fabricate coverage; replay opens a new session.
      await page.getByLabel('Position dans le titre',{exact:true}).focus();
      await page.getByLabel('Position dans le titre',{exact:true}).press('End');
      await page.waitForFunction(()=>[...document.querySelectorAll('audio')].some(a=>a.currentSrc.includes('/music/tracks/')&&a.ended));
      await page.getByRole('button',{name:'Lire Statistics Song',exact:true}).click();
      await page.waitForFunction(()=>[...document.querySelectorAll('audio')].some(a=>a.currentSrc.includes('/music/tracks/')&&!a.paused&&a.currentTime>1));
      await page.getByRole('button',{name:'Arrêter et vider la file',exact:true}).click();
      const music=await waitRead('/api/v1/account/statistics?kind=music',data=>data.summary.plays>=2&&data.summary.seconds>0);
      assert(music.summary.seconds>0);assert.equal(music.music.artist[0].label,'Statistics Artist');report.music=true;
      assert(music.sessions.some(s=>s.endReason==='ended'&&!s.completed));report.seekDoesNotComplete=true;
      await page.goto(`${origin}/app/statistics/`);await page.getByRole('heading',{name:'Mes statistiques',exact:true}).waitFor();
      await page.getByRole('button',{name:'Détail de Statistics Song'}).first().click();
      await page.getByRole('dialog').locator('.timeline li').first().waitFor();
      await page.getByRole('dialog').screenshot({path:`${out}/session.png`});
      await page.getByRole('dialog').getByRole('button',{name:'Fermer',exact:true}).click();
      await page.screenshot({path:`${out}/personal.png`,fullPage:true});
      await page.setViewportSize({width:390,height:844});
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
      await page.screenshot({path:`${out}/personal-mobile.png`,fullPage:true});
      await page.setViewportSize({width:1440,height:1000});
      await page.goto(`${origin}/app/admin/libraries/#statistics-section`);
      await page.getByRole('heading',{name:'Statistiques',exact:true}).waitFor();
      await page.screenshot({path:`${out}/admin.png`,fullPage:true});
      await page.getByText('Collecte et conservation',{exact:true}).click();await page.getByLabel('Activer les statistiques détaillées').uncheck();
      await page.getByRole('button',{name:'Enregistrer',exact:true}).click();await page.getByText('Paramètres enregistrés.',{exact:true}).waitFor();
      const before=(await read('/api/v1/admin/statistics')).summary.plays;
      await page.goto(`${origin}/app/music/`);await page.getByRole('button',{name:'Lire Statistics Song',exact:true}).click();
      await page.waitForFunction(()=>[...document.querySelectorAll('audio')].some(a=>!a.paused&&a.currentTime>1));
      await page.getByRole('button',{name:'Arrêter et vider la file',exact:true}).click();
      assert.equal((await read('/api/v1/admin/statistics')).summary.plays,before);report.disabledPlayback=true;
    }
    assert.deepEqual(errors,[]);report.passed=true;
  }catch(error){report.failure=String(error.stack);report.url=page.url();try{const stats=await read('/api/v1/admin/statistics');const detail=stats.sessions[0]?await read(`/api/v1/admin/statistics/sessions/${stats.sessions[0].id}`):null;report.diagnostics={behavior:stats.behavior,sessions:stats.sessions.map(s=>({kind:s.kind,seconds:s.seconds,endReason:s.endReason})),events:detail?.events.filter(r=>r.event.kind!=='heartbeat').map(r=>r.event),subtitlePreference:(await read('/api/v1/preferences')).subtitlePreference};}catch{}await page.screenshot({path:`${out}/failure.png`,fullPage:true}).catch(()=>{});process.exitCode=1;}
  finally{await browser.close();fs.writeFileSync(`${out}/report${restart?'-restart':''}.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {firefoxAdapter}=require('./browser-matrix.cjs');
const out=process.env.MEDIA_REPORT_DIR||'artifacts/immersive-firefox';fs.mkdirSync(out,{recursive:true});
(async()=>{
 let browser;const report={results:[],headless:true};
 try{
  browser=await firefoxAdapter();report.browser=browser.version;report.driver=browser.driver;
  await browser.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');
  await browser.evaluate(async()=>{
    const until=async(fn,label)=>{const end=Date.now()+10000;while(!fn()){if(Date.now()>end)throw new Error('Timeout '+label+' '+document.querySelector('[role=alert]')?.textContent);await new Promise(r=>setTimeout(r,20));}};
    const ready=()=>until(()=>document.querySelector('dialog').open&&!document.querySelector('button[aria-label="Audio"]').disabled&&!document.querySelector('.loading'),'ready');
    const wake=()=>document.querySelector('.player-shell').dispatchEvent(new PointerEvent('pointermove',{pointerType:'mouse',bubbles:true}));
    window.immersiveTest={until,ready,wake};await until(()=>document.querySelectorAll('.version-choice').length===2,'catalogue');
  });
  await browser.click('.hero-cta');await browser.evaluate(()=>window.immersiveTest.ready());
  const opening=await browser.evaluate(()=>({fullscreen:document.fullscreenElement===document.querySelector('.player-shell'),nativeControls:document.querySelector('video').controls,playing:!document.querySelector('video').paused}));assert(opening.fullscreen&&opening.playing&&!opening.nativeControls);report.results.push({opening});
  await browser.evaluate(()=>window.immersiveTest.until(()=>document.querySelector('.player-controls').classList.contains('concealed'),'auto hide'));await browser.evaluate(()=>window.immersiveTest.wake());await browser.click('button.primary');
  await browser.evaluate(()=>{const s=document.querySelector('input[aria-label="Position"]');s.value=4.25;s.dispatchEvent(new Event('input',{bubbles:true}));});
  await browser.click('[data-menu="audio"]');await browser.click('[data-audio-id="audio-en"]');await browser.evaluate(()=>window.immersiveTest.ready());assert.match(await browser.evaluate(()=>document.querySelector('video').currentSrc),/mkv-en/);
  await browser.click('[data-menu="subtitles"]');await browser.click('[data-subtitle-id="fr-forced"]');await browser.evaluate(()=>window.immersiveTest.until(()=>Array.from(document.querySelector('video').textTracks).some(t=>Array.from(t.activeCues||[]).some(c=>c.text==='FR FORCE CUE 2')),'forced cue'));
  await browser.click('[data-menu="video"]');await browser.click('[data-source-id="demo-mp4"]');await browser.evaluate(()=>window.immersiveTest.ready());const changed=await browser.evaluate(()=>({src:document.querySelector('video').currentSrc,position:document.querySelector('video').currentTime,paused:document.querySelector('video').paused}));assert.match(changed.src,/mp4-en/);assert(Math.abs(changed.position-4.25)<.15&&changed.paused);report.results.push({sourceAndAudioChange:changed,forcedSubtitle:true});
  await browser.evaluate(()=>document.querySelector('.player-shell').focus());await browser.press('m');assert.equal(await browser.evaluate(()=>document.querySelector('video').volume),0);await browser.press('m');await browser.press('ArrowDown');assert.equal(await browser.evaluate(()=>document.querySelector('video').volume),.95);await browser.press('ArrowRight');await browser.press('ArrowLeft');assert(Math.abs(await browser.evaluate(()=>document.querySelector('video').currentTime)-4.25)<.15);
  await browser.press('Space');await browser.evaluate(()=>window.immersiveTest.until(()=>!document.querySelector('video').paused,'play'));await browser.press('Space');await browser.evaluate(()=>window.immersiveTest.until(()=>document.querySelector('video').paused,'pause'));report.results.push({realWebDriverKeyboard:true});
  await browser.press('f');await browser.evaluate(()=>window.immersiveTest.until(()=>!document.fullscreenElement,'exit fullscreen'));await browser.click('[data-menu="audio"]');await browser.press('ArrowDown');await browser.press('Escape');assert(await browser.evaluate(()=>!document.querySelector('[role=menu]')&&document.querySelector('dialog').open));
  await browser.click('[data-menu="video"]');await browser.screenshot('desktop-menu.png');await browser.click('[aria-label="Fermer le menu"]');await browser.click('.return');await browser.evaluate(()=>window.immersiveTest.until(()=>!document.querySelector('dialog').open&&document.activeElement===document.querySelector('.hero-cta'),'focus return'));assert(await browser.evaluate(()=>!document.querySelector('video').getAttribute('src')&&document.querySelector('video').paused));report.results.push({escapeMenuAndReturnCleanup:true});
  await browser.evaluate(()=>{Element.prototype.requestFullscreen=()=>Promise.reject(new Error('test denial'));});await browser.click('.hero-cta');await browser.evaluate(()=>window.immersiveTest.ready());assert(await browser.evaluate(()=>document.querySelector('dialog').open&&!document.fullscreenElement&&document.querySelector('.fullscreen-notice').textContent.includes('mode immersif')));await browser.click('.return');report.results.push({fullscreenDeniedFallback:true});report.passed=true;
 }catch(e){report.failure=String(e.stack||e);process.exitCode=1;}finally{if(browser)await browser.close();fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

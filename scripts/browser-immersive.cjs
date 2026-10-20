const {chromium}=require('playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const out=process.env.MEDIA_REPORT_DIR||'artifacts/immersive';fs.mkdirSync(out,{recursive:true});
const channel=process.env.MEDIA_BROWSER_CHANNEL||'chrome';
async function ready(page){await page.waitForFunction(()=>document.querySelector('dialog')?.open&&!document.querySelector('button[aria-label="Audio"]')?.disabled&&!document.querySelector('.loading'));}
async function open(page){await page.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');await page.locator('.version-choice').first().waitFor();await page.locator('.hero-cta').click();await ready(page);}
async function wake(page){await page.locator('.player-shell').dispatchEvent('pointermove',{pointerType:'mouse'});}
async function pick(page,menu,selector){await wake(page);await page.locator(`[data-menu="${menu}"]`).click();await page.locator(selector).click();await ready(page);}
(async()=>{
  const browser=await chromium.launch({channel,executablePath:process.env.MEDIA_BROWSER_EXECUTABLE||undefined,headless:true});
  const report={browser:browser.version(),channel,results:[],headless:true};
  const push=report.results.push.bind(report.results);report.results.push=value=>{console.log('Checked',Object.keys(value));return push(value);};
  try{
    const page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(12000);const errors=[];page.on('pageerror',e=>errors.push(String(e)));
    await open(page);
    const initial=await page.evaluate(()=>{const d=document.querySelector('dialog'),s=document.querySelector('.player-shell'),v=document.querySelector('video'),r=s.getBoundingClientRect();return{fullscreen:document.fullscreenElement===s,dialog:d.open,ratio:[r.width,r.height,innerWidth,innerHeight],nativeControls:v.controls,playing:!v.paused};});
    assert(initial.dialog&&initial.playing&&!initial.nativeControls);assert.equal(initial.ratio[0],initial.ratio[2]);assert.equal(initial.ratio[1],initial.ratio[3]);assert(initial.fullscreen,'Actual fullscreen not entered');report.results.push({opening:initial});
    await page.waitForFunction(()=>document.querySelector('.player-controls').classList.contains('concealed'),{},{timeout:6000});
    assert.equal(await page.locator('.player-shell').evaluate(el=>getComputedStyle(el).cursor),'none');
    assert(await page.evaluate(()=>document.activeElement===document.querySelector('.player-shell')),'Focus lost after controls concealed');
    await page.keyboard.press('m');assert.equal(await page.locator('video').evaluate(v=>v.volume),0);
    await page.keyboard.press('m');assert.equal(await page.locator('video').evaluate(v=>v.volume),1);
    report.results.push({concealedFocusAndShortcuts:true});
    await wake(page);await page.waitForFunction(()=>!document.querySelector('.player-controls').classList.contains('concealed'));report.results.push({autoHideAndWake:true});
    await page.getByRole('button',{name:'Pause',exact:true}).click();await page.waitForTimeout(3300);assert(await page.locator('.player-controls').evaluate(el=>!el.classList.contains('concealed')));
    await page.getByLabel('Position',{exact:true}).fill('4.25');await page.waitForFunction(()=>Math.abs(document.querySelector('video').currentTime-4.25)<.15);
    await pick(page,'audio','[data-audio-id="audio-en"]');assert.match(await page.locator('video').evaluate(v=>v.currentSrc),/mkv-en/);assert(await page.locator('video').evaluate(v=>v.paused));
    await pick(page,'subtitles','[data-subtitle-id="fr-forced"]');await page.waitForFunction(()=>Array.from(document.querySelector('video').textTracks).some(t=>Array.from(t.activeCues||[]).some(c=>c.text==='FR FORCE CUE 2')));
    await pick(page,'video','[data-source-id="demo-mp4"]');assert.match(await page.locator('video').evaluate(v=>v.currentSrc),/mp4-en/);assert(Math.abs(await page.locator('video').evaluate(v=>v.currentTime)-4.25)<.15);assert(await page.locator('video').evaluate(v=>v.paused));
    await page.waitForFunction(()=>Array.from(document.querySelector('video').textTracks).some(t=>Array.from(t.activeCues||[]).some(c=>c.text==='FR FORCE CUE 2')));report.results.push({sourceSwitch:'MKV to MP4, position, pause, English and manual forced subtitle preserved'});
    await pick(page,'speed','[data-speed="1.5"]');assert.equal(await page.locator('video').evaluate(v=>v.playbackRate),1.5);
    await page.locator('.player-shell').focus();await page.keyboard.press('m');assert.equal(await page.locator('video').evaluate(v=>v.volume),0);await page.keyboard.press('m');assert.equal(await page.locator('video').evaluate(v=>v.volume),1);
    await page.keyboard.press('ArrowDown');assert.equal(await page.locator('video').evaluate(v=>v.volume),.95);await page.keyboard.press('ArrowUp');assert.equal(await page.locator('video').evaluate(v=>v.volume),1);
    await page.keyboard.press('ArrowRight');assert(await page.locator('video').evaluate(v=>v.currentTime>9));await page.keyboard.press('ArrowLeft');assert(Math.abs(await page.locator('video').evaluate(v=>v.currentTime)-4.25)<.15);
    await page.keyboard.press('Space');await page.waitForFunction(()=>!document.querySelector('video').paused);await ready(page);await page.keyboard.press('Space');await page.waitForFunction(()=>document.querySelector('video').paused);
    await page.getByLabel('Volume',{exact:true}).focus();const before=await page.locator('video').evaluate(v=>v.volume);await page.keyboard.press('ArrowLeft');assert(Math.abs(await page.locator('video').evaluate(v=>v.volume)-(before-.01))<.001);report.results.push({keyboard:'Space, seek, volume, mute, no range interception'});
    await wake(page);await page.locator('[data-menu="audio"]').click();assert(await page.getByRole('menu',{name:'Langue audio'}).isVisible());
    assert.equal(await page.locator('[role="menu"]').getAttribute('id'),'player-menu');
    assert.equal(await page.locator('[data-menu="audio"]').getAttribute('aria-controls'),'player-menu');
    assert(await page.locator('[role="menu"] [role="group"]').count()>=1);
    assert.equal(await page.locator('[role="menuitemradio"][aria-checked="true"]').getAttribute('tabindex'),'0');
    await page.keyboard.press('ArrowUp');await page.keyboard.press('ArrowDown');assert(await page.locator('[role="menuitemradio"]:focus').count());
    assert.equal(await page.locator('[role="menuitemradio"]:focus').getAttribute('tabindex'),'0');
    // DOM dispatch exercises app Escape without the browser consuming it to leave fullscreen first.
    await page.locator('[role="menu"]').dispatchEvent('keydown',{key:'Escape',bubbles:true});assert.equal(await page.getByRole('menu').count(),0);assert(await page.locator('dialog').evaluate(d=>d.open));report.results.push({menuKeyboard:true});
    await page.locator('.player-shell').focus();await page.keyboard.press('f');await page.waitForFunction(()=>!document.fullscreenElement);await page.keyboard.press('f');await page.waitForFunction(()=>!!document.fullscreenElement);report.results.push({fullscreenToggle:true});
    await wake(page);await page.locator('[data-menu="video"]').click();assert.match(await page.getByRole('menu').textContent(),/320 × 180/);assert.match(await page.getByRole('menu').textContent(),/Sélection interne non disponible/);await page.screenshot({path:`${out}/desktop-menu.png`});await page.getByRole('button',{name:'Fermer le menu'}).click();
    await page.getByRole('button',{name:'Retour à l’accueil'}).click();await page.waitForFunction(()=>!document.querySelector('dialog').open&&!document.fullscreenElement);const closed=await page.locator('video').evaluate(v=>({src:v.getAttribute('src'),paused:v.paused,ready:v.readyState,tracks:v.querySelectorAll('track').length}));assert.equal(closed.src,null);assert(closed.paused);assert.equal(closed.tracks,0);await page.waitForFunction(()=>document.activeElement===document.querySelector('.hero-cta')); report.results.push({returnAndCleanup:closed});
    await page.locator('.hero-cta').click();await ready(page);assert.match(await page.locator('video').evaluate(v=>v.currentSrc),/mkv-en/);report.results.push({manualLanguageAcrossReopen:true});await page.getByRole('button',{name:'Retour à l’accueil'}).click();assert.deepEqual(errors,[]);await page.close();
    for(const viewport of [{width:390,height:844},{width:844,height:390}]){
      const context=await browser.newContext({viewport,hasTouch:true,isMobile:true});context.setDefaultTimeout(12000);const touch=await context.newPage();await touch.addInitScript(()=>{Element.prototype.requestFullscreen=()=>Promise.reject(new Error('simulated denial'));});await open(touch);assert(!await touch.evaluate(()=>!!document.fullscreenElement));assert.match(await touch.locator('.fullscreen-notice').textContent(),/mode immersif/);
      const smallFontSize = await touch.locator('.title small').evaluate(el=>parseFloat(getComputedStyle(el).fontSize));assert(smallFontSize>=10,'Title small size must be >= 10px on mobile');
      const volDisplay = await touch.locator('.volume').evaluate(el=>getComputedStyle(el).display);assert.equal(volDisplay,'none','Volume slider must be hidden on mobile');
      await wake(touch);await touch.getByRole('button',{name:'Pause',exact:true}).tap();await touch.locator('[data-menu="audio"]').tap();await touch.locator('[data-audio-id="audio-en"]').tap();await ready(touch);assert.match(await touch.locator('video').evaluate(v=>v.currentSrc),/mkv-en/);
      await touch.locator('[data-menu="subtitles"]').tap();const rect=await touch.getByRole('menu').boundingBox();assert(rect.x>=0&&rect.y>=0&&rect.x+rect.width<=viewport.width+1&&rect.y+rect.height<=viewport.height+1);await touch.locator('[data-subtitle-id="en-full"]').tap();await ready(touch);
      assert(await touch.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await touch.screenshot({path:`${out}/touch-${viewport.width}.png`});await touch.getByRole('button',{name:'Retour à l’accueil'}).tap();report.results.push({touch:viewport,fallback:true,menusInViewport:true});await context.close();
    }
    report.passed=true;
  }catch(e){report.failure=String(e.stack||e);process.exitCode=1;}finally{await browser.close();fs.writeFileSync(`${out}/report.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

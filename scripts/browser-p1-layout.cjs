const {chromium}=require('playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
async function ready(page){await page.waitForFunction(()=>document.querySelector('dialog').open&&!document.querySelector('button[aria-label="Audio"]').disabled&&!document.querySelector('.loading'));}
async function open(page){await page.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');await page.locator('.version-choice').first().waitFor();await page.locator('.hero-cta').click();await ready(page);}
(async()=>{
  const out=process.env.MEDIA_REPORT_DIR||'artifacts/p1-layout';fs.mkdirSync(out,{recursive:true});
  const browser=await chromium.launch({channel:'chrome',headless:true});const report={browser:browser.version(),tests:[]};
  try{
    for(const width of [360,390,768,1280,1440,1920]){
      const page=await browser.newPage({viewport:{width,height:900}});const errors=[];
      page.on('pageerror',e=>errors.push(String(e)));page.on('console',msg=>{if(msg.type()==='error'&&/Content Security Policy/i.test(msg.text()))errors.push(msg.text());});
      await page.addInitScript(()=>{Element.prototype.requestFullscreen=()=>Promise.reject(new Error('test window size'));});
      await open(page);await page.keyboard.press('Tab');assert(await page.evaluate(()=>document.activeElement!==document.body));
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Horizontal overflow');
      assert(await page.locator('.brand img').evaluate(img=>img.complete && img.naturalWidth>0),'Brand image unavailable');
      assert.equal(await page.locator('.version-choice').count(),2,'Both fixture versions available');
      await page.screenshot({path:`${out}/${width}.png`});assert.deepEqual(errors,[]);report.tests.push({width,overflow:false,cspErrors:errors,keyboardFocus:true});await page.close();
    }
    const cards=await browser.newPage();await cards.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');
    await cards.getByRole('button',{name:'Sélectionner la version MP4'}).click();assert.equal(await cards.getByRole('button',{name:'Sélectionner la version MP4'}).getAttribute('aria-pressed'),'true');
    await cards.locator('.hero-cta').click();await ready(cards);assert.match(await cards.locator('video').evaluate(v=>v.currentSrc),/demo-mp4-fr/);
    report.tests.push({mediaCards:'MP4 selection and real source loaded'});await cards.close();
    const page=await browser.newPage();let attempts=0;
    await page.route('**/api/v1/assets/demo-mkv-en/content',route=>{attempts++;return route.fulfill({status:404,body:'missing'});});
    await open(page);await page.locator('[data-menu="audio"]').click();await page.locator('[data-audio-id="audio-en"]').click();await page.getByRole('alert').waitFor();
    assert(await page.locator('video').evaluate(v=>v.paused));await page.waitForTimeout(500);assert.equal(attempts,1);report.tests.push({missingAudio:'visible terminal error',attempts});
    await page.unroute('**/api/v1/assets/demo-mkv-en/content');await page.getByRole('button',{name:'Réessayer explicitement'}).click();await ready(page);
    await page.route('**/api/v1/demo/subtitles/fr-full.vtt',route=>route.fulfill({status:404,body:'missing'}));await page.locator('[data-menu="subtitles"]').click();await page.locator('[data-subtitle-id="fr-full"]').click();await page.getByRole('alert').waitFor();assert.match(await page.getByRole('alert').textContent(),/WebVTT/);report.tests.push({missingSubtitle:'visible error'});await page.close();
    const offline=await browser.newPage();await offline.route('**/api/v1/demo/media',r=>r.fulfill({status:503,body:'unavailable'}));await offline.goto('http://127.0.0.1:3000/app/films/?work=fixture-film');await offline.getByRole('alert').waitFor();report.tests.push({coreUnavailable:'visible error'});await offline.close();report.passed=true;
  }catch(e){report.failure=String(e.stack);process.exitCode=1;}finally{await browser.close();fs.writeFileSync(`${out}/report.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

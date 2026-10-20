const {chromium}=require('playwright');
const assert=require('node:assert/strict'),fs=require('node:fs');
const out=process.env.MEDIA_REPORT_DIR||'docs/evidence/2026-10-15/profile-navigation';fs.mkdirSync(out,{recursive:true});
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});const report={browser:browser.version(),os:require('node:os').release(),headless:true,results:[]};
 try{
  for(const width of [360,390,768,1440]){
   const context=await browser.newContext({viewport:{width,height:900},hasTouch:width<700,isMobile:width<700});const page=await context.newPage();const errors=[];
   page.on('pageerror',e=>errors.push(String(e)));page.on('console',m=>{if(m.type()==='error'&&/Content Security Policy/i.test(m.text()))errors.push(m.text());});
   await page.goto('http://127.0.0.1:3000/app/');await page.locator('.media-card').first().waitFor();
   const nav=page.getByRole('navigation',{name:'Navigation principale',exact:true});assert.equal(await nav.getByRole('link').count(),4);assert.equal(await nav.getByRole('link',{name:'Paramètres'}).count(),0);
   const search=nav.getByRole('link',{name:'Recherche',exact:true});assert.equal(await search.locator('svg').count(),1);assert(await search.isVisible());
   const trigger=page.getByRole('button',{name:'Menu profil',exact:true}),panel=page.locator('#profile-menu');const shown=()=>panel.evaluate(el=>el.matches(':popover-open'));
   await trigger.focus();await page.keyboard.press('Enter');await panel.waitFor({state:'visible'});
   await page.waitForFunction(()=>document.activeElement?.matches('#profile-menu a'));
   assert.equal(await trigger.getAttribute('aria-expanded'),'true');assert(await panel.getByRole('button',{name:'Mon profil Bientôt'}).isDisabled());assert.match(await panel.textContent(),/Aucun profil configuré/);
   assert.equal(await panel.getByText('Administration',{exact:true}).count(),0);assert.equal(await panel.getByText('Déconnexion',{exact:true}).count(),0);
   await page.keyboard.press('Escape');await panel.waitFor({state:'hidden'});await page.waitForFunction(()=>document.querySelector('.profile-trigger').getAttribute('aria-expanded')==='false');assert(await trigger.evaluate(el=>el===document.activeElement));
   await trigger.click();await panel.waitFor({state:'visible'});await trigger.click();await panel.waitFor({state:'hidden'});
   await trigger.focus();await page.keyboard.press('Space');await panel.waitFor({state:'visible'});await page.waitForFunction(()=>document.activeElement?.matches('#profile-menu a'));
   await page.keyboard.press('Tab');await panel.waitFor({state:'hidden'});
   if(width<700) await trigger.tap(); else await trigger.click();
   await panel.waitFor({state:'visible'});const rect=await panel.boundingBox();assert(rect.x>=0&&rect.y>=0&&rect.x+rect.width<=width+1&&rect.y+rect.height<=900);assert(await shown());
   await panel.evaluate(el=>Promise.all(el.getAnimations().map(animation=>animation.finished)));
   assert.equal(await panel.evaluate(el=>getComputedStyle(el).opacity),'1');
   await page.screenshot({path:`${out}/menu-${width}.png`,fullPage:width>700});
   if(width<700) await page.touchscreen.tap(8,300); else await page.mouse.click(8,300);await panel.waitFor({state:'hidden'});
   await trigger.click();await panel.waitFor({state:'visible'});await panel.getByRole('link',{name:'Paramètres',exact:true}).click();await page.getByRole('heading',{name:'Réglages personnels'}).waitFor();assert.match(page.url(),/\/settings\//);assert.equal(await page.getByText('Bibliothèques personnelles',{exact:true}).count(),0);assert(await page.getByRole('button',{name:'Réglages personnels · Bientôt'}).isDisabled());assert.equal(await panel.isVisible(),false);assert.equal(await nav.getByRole('link',{name:'Paramètres'}).count(),0);
   await trigger.click();await panel.waitFor({state:'visible'});await page.keyboard.press('Escape');
   await search.click();await page.getByRole('searchbox').waitFor();assert.equal(await search.getAttribute('aria-current'),'page');assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));assert.deepEqual(errors,[]);
   report.results.push({width,primaryLinks:4,searchIcon:true,profilePlaceholder:true,keyboardEnterSpaceTabEscape:true,focusRestored:true,toggleClose:true,outsideClick:true,settingsNavigation:true,menuInViewport:true,overflow:false,cspErrors:errors});await context.close();
  }
  report.passed=true;
 }catch(e){report.failure=String(e.stack);process.exitCode=1;}finally{await browser.close();fs.writeFileSync(`${out}/report.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

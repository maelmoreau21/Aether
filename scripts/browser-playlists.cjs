// Real Core/SQLite + signed-in Chrome, using synthetic media and disposable sessions.
const {chromium}=require('../apps/web/node_modules/playwright');
const assert=require('node:assert/strict');const fs=require('node:fs');
const origin=process.env.MEDIA_TEST_ORIGIN;
assert.match(origin||'',/^http:\/\/127\.0\.0\.1:\d+$/);
const out=process.env.MEDIA_REPORT_DIR||'target/playlists-browser';fs.mkdirSync(out,{recursive:true});
(async()=>{
  const browser=await chromium.launch({channel:'chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
  const report={browser:browser.version(),scope:'Synthetic music and disposable Core/SQLite accounts; no remote provider or physical audio output proof'};
  let page;
  async function context(token){const ctx=await browser.newContext({viewport:{width:1440,height:1000}});await ctx.addCookies([{name:'media_session',value:token.repeat(64),url:origin,httpOnly:true,sameSite:'Lax'}]);return ctx;}
  async function api(page,url,method='GET',body){return page.evaluate(async({url,method,body})=>{const me=await(await fetch('/api/v1/auth/me')).json();const response=await fetch(url,{method,headers:{'content-type':'application/json','x-csrf-token':me.csrfToken},body:body===undefined?undefined:JSON.stringify(body)});return {status:response.status,data:response.status===204?null:await response.json()};},{url,method,body});}
  try{
    const owner=await context('b');page=await owner.newPage();page.setDefaultTimeout(15000);
    const errors=[];page.on('pageerror',error=>errors.push(String(error)));
    await page.goto(origin+'/app/music/');
    await page.getByRole('button',{name:'Lire Song',exact:true}).click();
    await page.waitForFunction(()=>{const a=document.querySelector('audio');return a&&!a.paused&&a.currentTime>.2;});
    await page.getByRole('button',{name:'À suivre (2)',exact:true}).click();
    assert.equal(await page.locator('.music-player li').count(),2);
    await page.getByRole('button',{name:'Piste suivante',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('.music-player .now strong')?.textContent==='Next Song');
    await page.getByRole('button',{name:'Piste précédente',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('.music-player .now strong')?.textContent==='Song');
    await page.waitForFunction(()=>{const a=document.querySelector('audio');return a&&Number.isFinite(a.duration)&&a.duration>1&&a.readyState>=1;});
    await page.locator('audio').evaluate(audio=>{audio.currentTime=audio.duration-.2;});
    await page.waitForFunction(()=>document.querySelector('.music-player .now strong')?.textContent==='Next Song');
    await page.getByRole('button',{name:'Piste précédente',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('.music-player .now strong')?.textContent==='Song');report.automaticNextTitle=true;
    await page.getByRole('button',{name:'Paroles',exact:true}).click();
    await page.getByRole('button',{name:'Hello from the fixture',exact:true}).waitFor();
    await page.screenshot({path:out+'/music-lyrics.png'});
    const time=await page.locator('audio').evaluate(a=>a.currentTime);
    await page.getByRole('link',{name:'Playlists',exact:true}).click();
    await page.waitForFunction(time=>{const a=document.querySelector('audio');return a&&!a.paused&&a.currentTime>=time;},time);
    report.persistentPlayback=true;
    await page.getByRole('button',{name:'Paroles',exact:true}).click();
    await page.getByLabel('Nouvelle playlist',{exact:true}).fill('Mix browser');
    await page.getByRole('button',{name:'Créer',exact:true}).click();
    await page.getByRole('heading',{name:'Mix browser',exact:true}).waitFor();
    await page.getByLabel('Ajouter des médias',{exact:true}).fill('Song');
    await page.getByRole('button',{name:'Rechercher',exact:true}).click();
    await page.locator('.results>div').filter({hasText:'Song'}).first().getByRole('button',{name:'Ajouter',exact:true}).click();
    await page.locator('.entries li').first().waitFor();
    const created=(await api(page,'/api/v1/lists')).data.lists.find(list=>list.name==='Mix browser');assert.ok(created);
    assert.equal((await api(page,`/api/v1/lists/${created.id}/items/film`,'PUT')).status,204);
    await page.locator('.collection button').filter({hasText:'Mix browser'}).click();
    await page.waitForFunction(()=>document.querySelectorAll('.entries li').length===2);
    assert.equal(await page.locator('.entries li').count(),2);
    assert.equal((await api(page,`/api/v1/playlists/${created.id}/settings`,'PUT',{name:'Mix browser',visibility:'public',userIds:[]})).status,204);
    const publicPeer=await context('c');const publicPage=await publicPeer.newPage();await publicPage.goto(`${origin}/app/lists/?id=${created.id}`);
    await publicPage.getByRole('heading',{name:'Mix browser',exact:true}).waitFor();assert.equal(await publicPage.locator('.entries li').count(),1);await publicPeer.close();report.authenticatedPublicPlaylist=true;
    await page.getByText('Nom et partage',{exact:true}).click();
    await page.getByLabel('Visibilité',{exact:true}).selectOption('shared');
    await page.getByLabel('Rechercher une personne',{exact:true}).fill('peer');
    await page.locator('.settings').getByRole('button',{name:'Rechercher',exact:true}).click();
    await page.getByRole('button',{name:'+ peer',exact:true}).click();
    await page.locator('.settings').getByRole('button',{name:'Enregistrer',exact:true}).click();
    await page.getByText('Playlist enregistrée.',{exact:true}).waitFor();
    await page.screenshot({path:out+'/playlist-desktop.png'});
    await page.setViewportSize({width:390,height:844});await page.locator('.detail').scrollIntoViewIfNeeded();await page.screenshot({path:out+'/playlist-mobile.png'});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'mobile overflow');
    const peer=await context('c');const shared=await peer.newPage();await shared.goto(`${origin}/app/lists/?id=${created.id}`);
    await shared.getByRole('heading',{name:'Mix browser',exact:true}).waitFor();
    assert.equal(await shared.locator('.entries li').count(),1);assert.equal(await shared.getByText('Synthetic Film',{exact:true}).count(),0);
    assert.equal(await shared.getByText('Nom et partage',{exact:true}).count(),0);report.sharingPreservesLibraryPermissions=true;
    const guest=await browser.newContext();const anonymous=await guest.newPage();await anonymous.goto(`${origin}/app/lists/?id=${created.id}`);await anonymous.waitForURL('**/login/**');report.noAnonymousAccess=true;
    const admin=await context('a');const settings=await admin.newPage();await settings.goto(origin+'/app/admin/libraries/#general-section');
    await settings.getByRole('heading',{name:'Partage des playlists',exact:true}).waitFor();
    await settings.goto(origin+'/app/admin/libraries/#extensions-section');
    await settings.locator('#extensions-heading').waitFor();assert.equal(await settings.locator('input[type=file]').count(),0);
    await settings.getByRole('button',{name:/^Dépôts \(/}).click();await settings.getByLabel('Adresse du dépôt',{exact:true}).waitFor();
    assert.equal(await settings.getByRole('button',{name:/^Dépôts \(/}).getAttribute('aria-current'),'page');
    await settings.screenshot({path:out+'/extensions-repositories.png'});report.repositoryOnlyInterface=true;
    await settings.getByRole('button',{name:'Catalogue',exact:true}).click();await settings.getByLabel('Rechercher dans le catalogue',{exact:true}).waitFor();
    assert.equal((await api(settings,'/api/v1/admin/playlists/policy','PUT',{allowPublic:false,allowSharing:false})).status,204);
    await shared.reload();await shared.getByRole('alert').filter({hasText:'indisponible'}).waitFor();report.policyRevokesSharing=true;
    report.pcmPeak=await page.locator('audio').evaluate(async audio=>{
      const context=new AudioContext();await context.resume();const source=context.createMediaElementSource(audio),analyser=context.createAnalyser();source.connect(analyser);analyser.connect(context.destination);
      await new Promise(resolve=>setTimeout(resolve,400));const samples=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(samples);const peak=Math.max(...samples.map(Math.abs));source.disconnect();await context.close();return peak;
    });assert.ok(report.pcmPeak>.001,'decoded synthetic audio must be non-silent');
    assert.deepEqual(errors,[]);report.passed=true;
  }catch(error){report.failure=String(error.stack);if(page){report.diagnostic=await page.evaluate(()=>({text:document.body.innerText.slice(-6000),audio:{time:document.querySelector('audio')?.currentTime,paused:document.querySelector('audio')?.paused}})).catch(()=>null);await page.screenshot({path:out+'/failure.png'}).catch(()=>{});}process.exitCode=1;}
  finally{await browser.close();fs.writeFileSync(out+'/report.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report));}
})();

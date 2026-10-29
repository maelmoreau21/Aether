// Production Svelte UI with the real signed manifests; APIs are simulated here.
const {chromium}=require('../apps/web/node_modules/playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const catalog=JSON.parse(fs.readFileSync('../Aether-Plugins/repository/manifest.json','utf8'));
const origin=process.env.MEDIA_TEST_ORIGIN||'http://127.0.0.1:5179';
const output='target/extension-catalogue-ui';fs.mkdirSync(output,{recursive:true});
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:1000}});let theme='dark',available=false;
 let repositories=[{id:'community-default',name:'Aether Plugins',url:'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json',refreshedAt:0}],installed=[],packs=[];
 let chosenAvatar=false;const errors=[];page.on('pageerror',e=>errors.push(String(e)));const mutations=[];
 const libraries=[{id:'films',name:'Films',kind:'films',enabled:true,nodeId:'local',root:'/synthetic',userIds:[]}];
 try {
 await page.route('**/api/v1/**',async route=>{
  const r=route.request(),u=new URL(r.url()),p=u.pathname,method=r.method();let data={};
  if(method!=='GET')mutations.push({path:p,method});
  if(p==='/api/v1/auth/me')data={user:{id:'fixture-admin',login:'admin',role:'administrator'},csrfToken:'fixture'};
  else if(p==='/api/v1/admin/activity')data={sessions:[]};
  else if(p==='/api/v1/admin/api-keys')data={keys:[]};
  else if(p==='/api/v1/account/sessions')data={sessions:[]};
  else if(p==='/api/v1/account/avatar/extensions')data={extensions:installed.filter(e=>e.enabled&&e.manifest.permissions.includes('avatars')).map(e=>({id:e.manifest.id,name:e.manifest.name}))};
  else if(p==='/api/v1/account/avatar/collections')data={packs};
  else if(p==='/api/v1/account/avatar/collections/test-pack')data={images:[{id:'one',name:'Avatar témoin',url:'/api/v1/account/avatar/collections/test-pack/one'}]};
  else if(p==='/api/v1/account/avatar/collections/test-pack/one'||p==='/api/v1/account/avatar'){
   if(method==='POST'){chosenAvatar=true;await route.fulfill({status:204});return;}
   if(p.endsWith('/avatar')&&!chosenAvatar){await route.fulfill({status:404});return;}
   await route.fulfill({contentType:'image/png',body:Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=','base64')});return;
  }
  else if(p==='/api/v1/preferences')data={locale:'fr',theme,subtitles:'off',audioLanguage:'fr'};
  else if(p==='/api/v1/admin/extensions/repositories' && method==='GET')data={repositories};
  else if(p.endsWith('/community-default/refresh')){available=true;repositories[0].refreshedAt=1792742400;}
  else if(p.endsWith('/repositories/community-default')&&method==='DELETE'){repositories=[];available=false;}
  else if(p==='/api/v1/admin/extensions/catalogue')data={entries:available?catalog.extensions.map(e=>({...e,repositoryId:'community-default',repositoryName:'Aether Plugins',compatible:true})):[]};
  else if(p.endsWith('/community-default/install')){const body=r.postDataJSON(),e=catalog.extensions.find(e=>e.manifest.id===body.extensionId);installed.push({manifest:e.manifest,publisherId:e.publisherId,sha256:e.packageSha256,enabled:false,libraryIds:[],allLibraries:e.manifest.permissions.includes('metadata.read'),settings:{}});data={id:e.manifest.id};}
  else if(p.endsWith('/avatar-packs/upload')){assert(r.headers()['content-type'].startsWith('multipart/form-data; boundary='));assert(r.postDataBuffer().includes(Buffer.from('local.png')));packs.push({id:'upload-pack',name:'Images locales',count:1});data={count:1};}
  else if(p.endsWith('/avatar-packs/source'))data={folders:[{folder:'sonny',count:26}]};
  else if(p.endsWith('/avatar-packs')){if(method==='POST')packs.push({id:'test-pack',name:r.postDataJSON().name,count:26});data=method==='POST'?{count:26}:{packs};}
  else if(p.endsWith('/settings')){const entry=installed.find(e=>e.manifest.id===p.split('/').at(-2));if(method==='PUT')entry.settings=r.postDataJSON();data={settings:entry.settings};}
  else if(p==='/api/v1/admin/extensions')data={extensions:installed};
  else if(p==='/api/v1/admin/extensions/runs')data={runs:[]};
  else if(p==='/api/v1/admin/extensions/audit')data={audit:[]};
  else if(p.startsWith('/api/v1/admin/extensions/')&&method==='PUT'){const id=p.split('/').at(-1),body=r.postDataJSON(),entry=installed.find(e=>e.manifest.id===id);Object.assign(entry,body);}
  else if(p.endsWith('/libraries'))data={libraries};
  else if(p.endsWith('/users'))data={users:[]};
  else if(p.endsWith('/groups'))data={groups:[]};
  else if(p.endsWith('/roots'))data={roots:['/synthetic']};
  else if(p.endsWith('/nodes'))data={nodes:[]};
  else if(p.endsWith('/health'))data={status:'ok',mode:'standalone',version:'fixture'};
  else if(p.endsWith('/oidc'))data={enabled:false};
  await route.fulfill({json:data});
 });
 await page.goto(origin+'/app/admin/libraries/#extensions-section');const panel=page.locator('#extensions-section');

 const menu=page.locator('.mobile-menu-toggle');
 const openNavigation=async()=>{await page.waitForFunction(()=>innerWidth>=1100?document.querySelector('.desktop-sidebar'):document.querySelector('#admin-navigation'));if(await menu.isVisible() && await menu.getAttribute('aria-expanded')==='false')await menu.click();};
 await panel.getByRole('heading',{name:'Extensions',exact:true}).waitFor();
 await page.locator('.desktop-sidebar').waitFor();
 assert((await panel.boundingBox()).x>250,'Desktop content leaves room for persistent navigation');
 await page.setViewportSize({width:390,height:1000});
 await page.locator('#admin-navigation').waitFor({state:'attached'});
 assert.equal(await page.locator('#admin-navigation').isVisible(),false);
 assert((await panel.boundingBox()).x<70,'Mobile navigation leaves the content width available');
 await menu.click();await page.getByRole('dialog',{name:'Menu d’administration'}).waitFor();
 await page.keyboard.press('Shift+Tab');assert(await page.locator('#admin-navigation').evaluate(el=>el.contains(document.activeElement)));
 await page.keyboard.press('Escape');await page.waitForFunction(()=>document.querySelector('.mobile-menu-toggle').getAttribute('aria-expanded')==='false');assert.equal(await menu.getAttribute('aria-expanded'),'false');assert(await menu.evaluate(el=>el===document.activeElement));
 await menu.click();await page.mouse.click(380,500);assert.equal(await menu.getAttribute('aria-expanded'),'false');
 await menu.click();await page.locator('.admin-sidebar a[href$="#libraries-section"]').click();assert.equal(await menu.getAttribute('aria-expanded'),'false');

 await menu.click();await page.locator('.admin-sidebar a[href$="#overview"]').click();
 for(const width of [1440,390]){await page.setViewportSize({width,height:1000});await page.screenshot({path:`${output}/overview-${width}.png`,fullPage:true});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}
 await openNavigation();await page.locator('.admin-sidebar a[href$="#extensions-section"]').click();
 if(process.env.MEDIA_UI_NAVIGATION_ONLY==='1'){
  await panel.getByRole('heading',{name:'Extensions',exact:true}).waitFor();assert.deepEqual(errors,[]);
  const navigationReport={passed:true,browser:browser.version(),scope:'Admin navigation only; synthetic APIs. Extension installation/configuration/avatar flows were not run.',checks:['persistent desktop sidebar','mobile drawer','Escape and focus return','outside dismissal','navigation between extensions, libraries and overview','desktop/mobile resizing'],errors};
  fs.writeFileSync(output+'/navigation-report.json',JSON.stringify(navigationReport,null,2));console.log(JSON.stringify(navigationReport,null,2));return;
 }
 await panel.getByRole('button',{name:'Actualiser les dépôts',exact:true}).click();
 await panel.locator('.catalogue-card').nth(catalog.extensions.length-1).waitFor();assert.equal(await panel.locator('.catalogue-card').count(),catalog.extensions.length);assert(!/[├┬�]/u.test(await panel.innerText()));
 for(const current of ['dark','light']){
  theme=current;await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
  for(const width of [1440,390]){
   await page.setViewportSize({width,height:1000});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
   await page.screenshot({path:`${output}/catalogue-${theme}-${width}.png`,fullPage:true});
   await openNavigation();await page.screenshot({path:`${output}/menu-${theme}-${width}.png`,fullPage:true});if(await menu.isVisible())await page.getByRole('button',{name:'Fermer le menu',exact:true}).click();
  }
 }
 const avatar=panel.locator('.catalogue-card').filter({hasText:'Aether Avatars'});
 assert(await avatar.getByRole('button',{name:'Installer Aether Avatars',exact:true}).isEnabled());
 await avatar.getByRole('button',{name:'Installer Aether Avatars',exact:true}).click();
 await panel.locator('.extension-detail').waitFor();assert.equal(installed[0].enabled,false);
 assert.equal(await panel.getByText('Bibliothèques autorisées',{exact:true}).count(),0);
 await panel.getByRole('button',{name:'Parcourir les collections',exact:true}).click();
 await panel.getByLabel('Nom affiché',{exact:true}).fill('Sonny');
 await panel.getByRole('button',{name:'Importer la collection',exact:true}).click();
 await panel.getByText('26 avatars importés.',{exact:true}).waitFor();
 await panel.getByText('Importer depuis cet ordinateur',{exact:true}).click();
 await panel.getByLabel('Nom de la collection',{exact:true}).fill('Images locales');
 await panel.getByLabel('Images à envoyer',{exact:true}).setInputFiles({name:'local.png',mimeType:'image/png',buffer:Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=','base64')});
 await panel.getByRole('button',{name:'Envoyer au serveur',exact:true}).click();
 await panel.getByText('1 avatar envoyé au serveur.',{exact:true}).waitFor();
 await panel.getByRole('button',{name:'Enregistrer les paramètres',exact:true}).click();
 await panel.getByText('Paramètres enregistrés.',{exact:true}).waitFor();
 await panel.getByRole('switch',{name:'Activer Aether Avatars',exact:true}).check();
 await panel.getByText('Permissions enregistrées et extension activée.',{exact:true}).waitFor();assert.equal(installed[0].enabled,true);
 for(const current of ['dark','light']){await page.evaluate(t=>document.documentElement.dataset.theme=t,current);for(const width of [1440,390]){await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));if(width===390){assert((await panel.getByLabel('Lien GitHub de la collection',{exact:true}).boundingBox()).width>=200,'Readable mobile repository input');assert((await panel.getByLabel('Collection',{exact:true}).boundingBox()).width>=200,'Readable mobile collection selector');}await page.screenshot({path:`${output}/settings-${current}-${width}.png`,fullPage:true});}}
 await panel.getByRole('button',{name:'Catalogue',exact:true}).click();
 const tmdb=panel.locator('.catalogue-card').filter({has:page.getByRole('heading',{name:'TMDb',exact:true})});
 await tmdb.getByRole('button',{name:/^Installer/}).click();
 await panel.getByLabel('Utilisation',{exact:true}).waitFor();assert.equal(await panel.getByLabel('Utilisation',{exact:true}).inputValue(),'true');
 await panel.getByRole('switch',{name:'Activer TMDb',exact:true}).check();await panel.getByText('Permissions enregistrées et extension activée.',{exact:true}).waitFor();assert.equal(installed[1].allLibraries,true);
 await panel.getByRole('button',{name:'Dépôts',exact:true}).click();
 await panel.getByRole('button',{name:'Retirer le dépôt',exact:true}).click();
 await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();
 await page.reload();await panel.getByRole('button',{name:'Dépôts',exact:true}).click();await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();assert.equal(installed.length,2);
 assert(!mutations.some(m=>m.path.includes("publishers")));
 await page.goto(origin+'/app/settings/#account');
 await page.getByRole('combobox',{name:/Collections du serveur/}).selectOption('test-pack');
 await page.getByRole('button',{name:'Choisir Avatar témoin',exact:true}).click();
 await page.getByText('Avatar enregistré.',{exact:true}).waitFor();assert(chosenAvatar);
 await page.screenshot({path:`${output}/profile-gallery-mobile.png`,fullPage:true});
 assert.deepEqual(errors,[]);
 const report={passed:true,browser:browser.version(),scope:'Production Svelte UI with actual signed manifests; simulated APIs. Real Core persistence/signatures/install verified separately.',cards:catalog.extensions.length,checks:['collapsed desktop/mobile navigation, Escape/focus/outside click','refresh pending default repository','dark/light 1440/390 no overflow','no publisher approval or publisher API calls','avatar collection import','multipart local image upload','profile gallery choice','dynamic schema save','metadata defaults to all libraries','individual install disabled','explicit activation','remove repository preserves installation'],mutations,errors};
 fs.writeFileSync(output+'/report.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
 }catch(error){console.error('Browser errors:',errors);await page.screenshot({path:output+'/failure.png',fullPage:true});throw error;}
 finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});


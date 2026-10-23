// Production Svelte UI with the real four signed manifests; APIs are simulated here.
const {chromium}=require('../apps/web/node_modules/playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const catalog=JSON.parse(fs.readFileSync('../Aether-Plugins/repository/manifest.json','utf8'));
const origin=process.env.MEDIA_TEST_ORIGIN||'http://127.0.0.1:5179';
const output='target/extension-catalogue-ui';fs.mkdirSync(output,{recursive:true});
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:1000}});let theme='dark',available=false;
 let repositories=[{id:'community-default',name:'Aether Plugins',url:'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json',refreshedAt:0}],installed=[],publishers=[];
 const errors=[];page.on('pageerror',e=>errors.push(String(e)));const mutations=[];
 const libraries=[{id:'films',name:'Films',kind:'films',enabled:true,nodeId:'local',root:'/synthetic',userIds:[]}];
 try {
 await page.route('**/api/v1/**',async route=>{
  const r=route.request(),u=new URL(r.url()),p=u.pathname,method=r.method();let data={};
  if(method!=='GET')mutations.push({path:p,method});
  if(p==='/api/v1/auth/me')data={user:{id:'fixture-admin',login:'admin',role:'administrator'},csrfToken:'fixture'};
  else if(p==='/api/v1/preferences')data={locale:'fr',theme,subtitles:'off',audioLanguage:'fr'};
  else if(p==='/api/v1/admin/extensions/repositories' && method==='GET')data={repositories};
  else if(p.endsWith('/community-default/refresh')){available=true;repositories[0].refreshedAt=1792742400;}
  else if(p.endsWith('/repositories/community-default')&&method==='DELETE'){repositories=[];available=false;}
  else if(p==='/api/v1/admin/extensions/catalogue')data={entries:available?catalog.extensions.map(e=>({...e,repositoryId:'community-default',repositoryName:'Aether Plugins',compatible:true})):[]};
  else if(p==='/api/v1/admin/extensions/publishers'){
   if(method==='POST'){const body=r.postDataJSON();publishers.push({...body,active:true});}data={publishers};
  }
  else if(p.endsWith('/community-default/install')){const body=r.postDataJSON(),e=catalog.extensions.find(e=>e.manifest.id===body.extensionId);assert(publishers.some(p=>p.id===e.publisherId));installed.push({manifest:e.manifest,publisherId:e.publisherId,sha256:e.packageSha256,enabled:false,libraryIds:[],settings:{}});data={id:e.manifest.id};}
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

 const menu=page.getByRole('button',{name:'Menu',exact:true});
 await panel.getByRole('heading',{name:'Extensions',exact:true}).waitFor();
 assert.equal(await page.locator('#admin-navigation').isVisible(),false);
 assert((await panel.boundingBox()).x<70,'No permanent sidebar space');
 await menu.click();await page.getByRole('dialog',{name:'Menu d’administration'}).waitFor();
 await page.keyboard.press('Shift+Tab');assert(await page.locator('#admin-navigation').evaluate(el=>el.contains(document.activeElement)));
 await page.keyboard.press('Escape');await page.waitForFunction(()=>document.querySelector('.mobile-menu-toggle').getAttribute('aria-expanded')==='false');assert.equal(await menu.getAttribute('aria-expanded'),'false');assert(await menu.evaluate(el=>el===document.activeElement));
 await menu.click();await page.mouse.click(800,500);assert.equal(await menu.getAttribute('aria-expanded'),'false');
 await menu.click();await page.locator('.admin-sidebar a[href$="#libraries-section"]').click();assert.equal(await menu.getAttribute('aria-expanded'),'false');

 await menu.click();await page.locator('.admin-sidebar a[href$="#overview"]').click();
 for(const width of [1440,390]){await page.setViewportSize({width,height:1000});await page.screenshot({path:`${output}/overview-${width}.png`,fullPage:true});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}
 await menu.click();await page.locator('.admin-sidebar a[href$="#extensions-section"]').click();
 await panel.getByRole('button',{name:'Actualiser les dépôts',exact:true}).click();
 await panel.locator('.catalogue-card').nth(3).waitFor();assert.equal(await panel.locator('.catalogue-card').count(),4);
 for(const current of ['dark','light']){
  theme=current;await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
  for(const width of [1440,390]){
   await page.setViewportSize({width,height:1000});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
   await page.screenshot({path:`${output}/catalogue-${theme}-${width}.png`,fullPage:true});
   await menu.click();await page.screenshot({path:`${output}/menu-${theme}-${width}.png`,fullPage:true});await page.getByRole('button',{name:'Fermer le menu',exact:true}).click();
  }
 }
 const avatar=panel.locator('.catalogue-card').filter({hasText:'Aether Avatars'});
 assert(await avatar.getByRole('button',{name:'Installer Aether Avatars',exact:true}).isDisabled());
 await avatar.getByText('Vérifier l’éditeur maelmoreau21',{exact:true}).click();
 await avatar.getByRole('button',{name:'J’ai vérifié cette clé : approuver',exact:true}).click();
 await avatar.getByRole('button',{name:'Installer Aether Avatars',exact:true}).click();
 await panel.locator('.installed-card').waitFor();assert.equal(installed[0].enabled,false);
 await panel.getByText('Configurer et gérer',{exact:true}).click();
 await panel.getByRole('button',{name:'Enregistrer et activer',exact:true}).click();
 await panel.getByText('Permissions enregistrées et extension activée.',{exact:true}).waitFor();assert.equal(installed[0].enabled,true);
 await page.screenshot({path:`${output}/installed-mobile.png`,fullPage:true});
 await panel.getByRole('button',{name:'Dépôts',exact:true}).click();
 await panel.getByRole('button',{name:'Retirer le dépôt',exact:true}).click();
 await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();
 await page.reload();await panel.getByRole('button',{name:'Dépôts',exact:true}).click();await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();assert.equal(installed.length,1);
 assert.deepEqual(errors,[]);
 const report={passed:true,browser:browser.version(),scope:'Production Svelte UI with actual signed manifests; simulated APIs. Real Core persistence/trust/install verified separately.',cards:4,checks:['collapsed desktop/mobile navigation, Escape/focus/outside click','refresh pending default repository','dark/light 1440/390 no overflow','publisher approval required','individual install disabled','explicit activation','remove repository preserves installation'],mutations,errors};
 fs.writeFileSync(output+'/report.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
 }catch(error){await page.screenshot({path:output+'/failure.png',fullPage:true});throw error;}
 finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});


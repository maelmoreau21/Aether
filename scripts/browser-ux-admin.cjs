// Production admin UI. Every API response below is an explicit synthetic fixture.
const { chromium } = require('../apps/web/node_modules/playwright');
const { fixturePage } = require('./browser-ux-foundations.cjs');
const fs=require('node:fs'),assert=require('node:assert/strict');
const base=process.env.MEDIA_UI_URL||'http://127.0.0.1:5182/app/';
const out=process.env.MEDIA_REPORT_DIR||'target/ux-admin';fs.mkdirSync(out,{recursive:true});
const report={scope:'Production Svelte admin shell; synthetic intercepted APIs, no Core/NAS proof',scenarios:[],screenshots:[]};
const library={id:'lib-film',name:'Films de voyage — Bibliothèque personnelle',kind:'films',nodeId:'local',root:'/synthetic/films',enabled:true,userIds:['visual'],folders:[{nodeId:'local',root:'/synthetic/films'}]};
const users=[{id:'visual',login:'visual',displayName:'Administrateur de test',role:'administrator',sso:false,disabledAt:null},{id:'reader',login:'lecteur',displayName:'Compte de test',role:'user',sso:false,disabledAt:null}];
const node={id:'local',name:'Serveur local',kind:'local',status:'connected',keyId:'test',roots:['/synthetic/films'],version:'test',pairedAt:1,libraries:[],scans:[]};
const routes=[['overview','#overview'],['general-section','#general-section'],['network-section','#network-section'],['libraries-section','#libraries-section'],['artwork-section','#artwork-admin-title'],['nodes-section','#nodes-section'],['avatars-section','section[aria-label="Collections d’avatars"]'],['users-section','#users-section'],['groups-section','#groups-section'],['api-keys-section','#api-keys-section'],['tasks-section','#tasks-section'],['backups-section','#backups-section'],['extensions-section','#extensions-section']];
async function setup(browser,width,theme,mode='normal',height=900){
 const state=await fixturePage(browser,width,theme,'normal',height);let fail=mode==='error';const requests=[];
 state.page.on('request',request=>{if(request.url().includes('/api/v1/admin/'))requests.push(request.url());});
 if(mode==='user')await state.page.route('**/api/v1/auth/me',route=>route.fulfill({json:{user:users[1],csrfToken:'test'}}));
 if(mode==='english')await state.page.route('**/api/v1/preferences',route=>route.fulfill({json:{locale:'en',theme}}));
 if(mode==='diagnostics')await state.page.route('**/api/v1/health',route=>route.fulfill({status:503,json:{error:'synthetic_unavailable'}}));
 await state.page.route('**/api/v1/admin/**',async route=>{
  const p=new URL(route.request().url()).pathname;
  let data={users,groups:[],libraries:[library],nodes:[node],keys:[],packs:[],images:[],roots:['/synthetic/films'],localRoots:['/synthetic/films'],sessions:[],extensions:[],repositories:[],entries:[],tasks:[],jobs:[],policies:[],quotas:[],sources:[],limit:50,offset:0};
  if(mode==='diagnostics'&&['/api/v1/admin/nodes','/api/v1/admin/activity'].includes(p))return route.fulfill({status:503,json:{error:'synthetic_unavailable'}});
  if(p==='/api/v1/admin/libraries'){
   if(mode==='loading')await new Promise(resolve=>setTimeout(resolve,1000));
   if(fail){fail=false;return route.fulfill({status:503,json:{error:'synthetic_unavailable'}});}
   data={libraries:mode==='empty'?[]:[library]};
  }else if(p==='/api/v1/admin/users')data={users:mode==='empty'?[]:users};
  else if(p.endsWith('/scan'))data={status:'completed',discovered:2,probed:2,reused:0,errors:[]};
  else if(p==='/api/v1/admin/groups')data={groups:[]};
  else if(p==='/api/v1/admin/settings')data={serverName:'Aether',welcomeMessage:'',ssoButtonLabel:'Se connecter avec SSO',blockSsoLocalAccounts:true};
  else if(p==='/api/v1/admin/network')data={publicUrl:'http://localhost',secureCookies:false,webPath:'/app/',apiPath:'/api/v1'};
  else if(p==='/api/v1/admin/tasks/schedules'||p==='/api/v1/admin/tasks/system')data=[];
  else if(p==='/api/v1/admin/backups')data={available:false,reason:'Stockage non configuré dans ce banc',running:false,archives:[],schedule:{enabled:false,intervalMinutes:1440,nextDueAt:0,lastStartedAt:null,lastCompletedAt:null,lastOutcome:'never'}};
  else if(p==='/api/v1/admin/system/memory')return route.fulfill({status:503,json:{error:'metrics_unavailable'}});
  else if(p==='/api/v1/admin/avatar-policy')data={allowCustom:true,allowGravatar:false,assignOnCreate:false};
  else if(p==='/api/v1/admin/playlists/policy')data={allowSharing:false,allowPublic:false};
  return route.fulfill({json:data});
 });return {...state,requests};
}
async function shot(page,name){await page.screenshot({path:`${out}/${name}.png`});report.screenshots.push(name);}
async function bounds(page,label){
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,label+' document overflow');
 const clipped=await page.locator('.admin-header,.admin-content-area,.desktop-sidebar').evaluateAll(nodes=>nodes.filter(node=>{const r=node.getBoundingClientRect();return r.width>0&&(r.left<0||r.right>innerWidth+1);}).map(node=>node.className));assert.deepEqual(clipped,[],label+' clipped layout');
}
async function choose(page,hash){
 const menu=page.locator('.mobile-menu-toggle');if(await menu.isVisible()&&await menu.getAttribute('aria-expanded')==='false')await menu.click();
 await page.locator(`.admin-sidebar a[href$="#${hash}"]`).click();
 await page.waitForFunction(hash=>location.hash==='#'+hash,hash);
}
async function run(){const browser=await chromium.launch({channel:'chrome',headless:true});report.browser=browser.version();try{
 for(const theme of ['light','dark'])for(const width of [320,360,390,430,768,1024,1440,1920]){
  const {page,context,errors}=await setup(browser,width,theme);
  await page.goto(base+'admin/libraries/#overview');await page.locator('#overview').waitFor();
  if(width>=1100){await page.locator('.desktop-sidebar').waitFor();assert.equal(await page.locator('.mobile-menu-toggle').isVisible(),false);assert.equal(await page.locator('.admin-sidebar a').count(),13);}
  else{
   const menu=page.locator('.mobile-menu-toggle');await menu.focus();await page.keyboard.press('Enter');await page.locator('#admin-navigation:modal').waitFor();
   assert.equal(await page.locator('#admin-navigation a').count(),13);await shot(page,`${theme}-${width}-menu`);
   await page.keyboard.press('Escape');await page.waitForFunction(()=>document.querySelector('.mobile-menu-toggle').getAttribute('aria-expanded')==='false');assert(await menu.evaluate(el=>el===document.activeElement));
  }
  await bounds(page,'overview');await shot(page,`${theme}-${width}-overview`);
  for(const [hash,selector] of routes){
   await choose(page,hash);await page.locator(selector).waitFor();
   assert.equal(await page.locator(`.admin-sidebar a[aria-current="page"]`).count(),1);
   assert.equal(await page.evaluate(()=>document.activeElement.id),'admin-main-content');
   assert.equal(await page.evaluate(()=>scrollY),0);
   await bounds(page,hash);
   if(['libraries-section','users-section','tasks-section'].includes(hash))await shot(page,`${theme}-${width}-${hash}`);
  }
  await choose(page,'users-section');await page.locator('#users-section input[type=search]').fill('lecteur');await choose(page,'groups-section');await choose(page,'users-section');assert.equal(await page.locator('#users-section input[type=search]').inputValue(),'lecteur');
  await page.goBack();await page.locator('#groups-section').waitFor();await page.goForward();await page.locator('#users-section').waitFor();
  // The skip link must keep the selected section instead of navigating to overview.
  const selectedURL=page.url();await page.locator('.skip-admin').focus();await page.keyboard.press('Enter');assert.equal(page.url(),selectedURL);assert.equal(await page.evaluate(()=>document.activeElement.id),'admin-main-content');
  await choose(page,'libraries-section');await page.getByRole('button',{name:'Ajouter une bibliothèque',exact:true}).click();await page.getByRole('dialog',{name:'Ajouter une bibliothèque',exact:true}).waitFor();await page.keyboard.press('Escape');await page.waitForFunction(()=>!document.querySelector('.library-editor')?.open);
  assert.deepEqual(errors,[]);report.scenarios.push({theme,width,sections:13,history:true,filterPreserved:true,keyboard:true,editor:true,overflow:false});await context.close();console.log('PASS',theme,width);
 }
 for(const mode of ['empty','error','loading','user']){
  const{page,context,errors,requests}=await setup(browser,390,'dark',mode);await page.goto(base+'admin/libraries/#libraries-section');
  if(mode==='user'){await page.getByText('Accès refusé',{exact:true}).waitFor();assert.equal(await page.locator('.admin-sidebar').count(),0);assert.equal(requests.length,0);}
  else{if(mode==='loading'){await page.getByText('Chargement des données administratives…').waitFor();await shot(page,'state-loading');}await page.locator('#libraries-section').waitFor();if(mode==='error'){await page.locator('main > .error-banner').waitFor();await shot(page,'state-error');await page.locator('main > .error-banner').getByRole('button',{name:'Réessayer'}).click();await page.getByRole('button',{name:`Gérer ${library.name}`}).waitFor();}}
  await shot(page,'state-'+mode);await bounds(page,mode);assert.deepEqual(errors,[]);report.scenarios.push({mode,pass:true});await context.close();
 }
 const diagnostic=await setup(browser,320,'light','diagnostics');await diagnostic.page.goto(base+'admin/libraries/#overview');await diagnostic.page.locator('#overview').waitFor();await diagnostic.page.getByText('État des Media Nodes indisponible. Réessayez pour vérifier les serveurs.',{exact:true}).first().waitFor();assert.deepEqual(await diagnostic.page.locator('.overview-grid strong').allTextContents(),['—','—','—','0']);await shot(diagnostic.page,'state-diagnostics');await bounds(diagnostic.page,'diagnostics');assert.deepEqual(diagnostic.errors,[]);report.scenarios.push({diagnosticsUnavailable:true});await diagnostic.context.close();
 const english=await setup(browser,1440,'light','english');await english.page.goto(base+'admin/libraries/#network-section');await english.page.locator('#network-section').waitFor();await english.page.getByRole('link',{name:'Network',exact:true}).waitFor();assert((await english.page.title()).startsWith('Network'));await shot(english.page,'english-navigation');assert.deepEqual(english.errors,[]);report.scenarios.push({englishNavigation:true});await english.context.close();
 const{page,context,errors}=await setup(browser,390,'dark');await page.goto(base+'admin/libraries/#tasks-section?task=scan-lib-film');await page.getByRole('heading',{name:'Indexer '+library.name,exact:true}).waitFor();assert.equal(await page.locator('.admin-sidebar [aria-current=page]').getAttribute('href'),'/app/admin/libraries/#tasks-section');
 await page.locator('.mobile-menu-toggle').click();await page.setViewportSize({width:1440,height:900});await page.locator('.desktop-sidebar').waitFor();assert.equal(await page.locator('dialog:modal').count(),0);assert.notEqual(await page.evaluate(()=>getComputedStyle(document.body).overflowY),'hidden');await shot(page,'desktop-deep-task');
 await page.setViewportSize({width:844,height:390});await page.locator('.mobile-menu-toggle').tap();await page.locator('#admin-navigation:modal').waitFor();await bounds(page,'landscape');assert.ok(await page.locator('.drawer-heading button').isVisible());await shot(page,'landscape-menu');await page.getByRole('button',{name:'Fermer le menu',exact:true}).tap();
 await page.goto(base+'admin/libraries/#unknown');await page.locator('#overview').waitFor();assert.equal(await page.locator('.admin-sidebar [aria-current=page]').getAttribute('href'),'/app/admin/libraries/#overview');assert.deepEqual(errors,[]);report.scenarios.push({deepTask:true,resize:true,landscapeTouch:true,unknownHash:true});await context.close();
}finally{fs.writeFileSync(`${out}/report.json`,JSON.stringify(report,null,2));await browser.close();}console.log('Completed',report.scenarios.length,'admin scenarios');}
if(require.main===module)run().catch(error=>{console.error(error);process.exitCode=1;});
module.exports={setup};

const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const [origin,password,out] = process.argv.slice(2);
(async () => {
 const browser=await chromium.launch({channel:'chrome',headless:true});
 try {
  const context=await browser.newContext();const page=await context.newPage();const errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  page.on('response',async r=>{ if(r.status()>=400 && new URL(r.url()).pathname.startsWith('/api/')) console.error('HTTP',r.status(),new URL(r.url()).pathname); });
  page.on('console',m=>{ if(m.type()==='error' && /Content Security Policy/i.test(m.text())) errors.push(m.text()); });
  await page.goto(origin+'/app/login/');
  await page.getByRole('button',{name:'Se connecter avec SSO',exact:true}).click();
  await page.waitForURL(origin+'/app/');
  await page.getByRole('button',{name:'Menu profil',exact:true}).waitFor();
  let me=await context.request.get(origin+'/api/v1/auth/me');let data=await me.json();
  assert.equal(data.user.role,'user');const id=data.user.id;
  let cookies=await context.cookies();let cookie=cookies.find(c=>c.name==='media_session');assert(cookie.httpOnly && cookie.sameSite==='Strict');assert(!cookies.some(c=>c.name==='media_oidc_flow' && c.value));
  assert.equal(await page.evaluate(()=>Object.keys(localStorage).some(k=>/auth|token|oidc|session/i.test(k))),false);
  const logout=await page.evaluate(async csrf=>{const r=await fetch('/api/v1/auth/logout',{method:'POST',headers:{'x-csrf-token':csrf}});return r.status},data.csrfToken);assert.equal(logout,204);
  assert.equal((await context.request.get(origin+'/api/v1/auth/me')).status(),401);
  await page.goto(origin+'/app/login/');await page.getByRole('button',{name:'Se connecter avec SSO',exact:true}).click();await page.waitForURL(origin+'/app/');
  me=await context.request.get(origin+'/api/v1/auth/me');data=await me.json();assert.equal(data.user.id,id);
  await page.evaluate(async csrf=>fetch('/api/v1/auth/logout',{method:'POST',headers:{'x-csrf-token':csrf}}),data.csrfToken);
  await page.goto(origin+'/app/login/');await page.getByLabel('Identifiant',{exact:true}).fill('qa-admin');await page.getByLabel('Mot de passe',{exact:true}).fill(password);await page.getByRole('button',{name:'Se connecter',exact:true}).click();await page.waitForURL(origin+'/app/');
  const users=await (await context.request.get(origin+'/api/v1/admin/users')).json();assert(users.users.some(u=>u.id===id && u.login==='sso-'+id));
  await page.goto(origin+'/app/settings/');await page.getByText('SSO activé. La connexion locale reste disponible.',{exact:true}).waitFor();await page.getByText('docker-client',{exact:true}).waitFor();
  assert.equal((await page.content()).includes('synthetic-never-exposed'),false);
  assert.deepEqual(errors,[]);
  fs.writeFileSync(out,JSON.stringify({browser:browser.version(),os:require('node:os').release(),checks:['SSO button','PKCE code exchange','Core session cookie HttpOnly Strict','logout','same subject same user','local rescue login','admin status','no localStorage token','no JS error']}));
 } finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;});

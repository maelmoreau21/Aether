import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';

const origin = process.env.MEDIA_TEST_ORIGIN;
const repositoryUrl = process.env.MEDIA_TEST_REPOSITORY_URL;
if (!origin || !repositoryUrl) throw new Error('Use the ignored Rust repository browser harness.');
const output = process.env.MEDIA_REPORT_DIR || '../../target/extensions-repository-browser';
await mkdir(output,{recursive:true});
const browser = await chromium.launch({channel:'chrome',headless:true});
const page = await browser.newPage({viewport:{width:1440,height:1000}});
const errors=[];
page.on('pageerror',error=>errors.push(error.message));
try {
  const login=await page.request.post(`${origin}/api/v1/auth/login`,{headers:{origin},data:{login:'repo-browser',password:'repository browser fixture password'}});
  assert.equal(login.status(),200);
  await page.goto(`${origin}/app/admin/libraries/#extensions-section`);
  const panel=page.locator('#extensions-section');
  await panel.getByRole('button',{name:'Dépôts',exact:true}).click();
  const initialDefault = panel.locator('.repository-card').filter({hasText:'Aether Plugins'});
  if (await initialDefault.count()) {
    await initialDefault.getByRole('button',{name:'Retirer le dépôt',exact:true}).click();
    await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();
    await page.reload();
    await panel.getByRole('button',{name:'Dépôts',exact:true}).click();
    await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();
  }

  await panel.getByLabel('Nom du dépôt (facultatif)',{exact:true}).fill('Repository browser test');
  await panel.getByLabel('Adresse du dépôt',{exact:true}).fill('https://127.0.0.1/manifest.json');
  const rejected=page.waitForResponse(response=>response.url().endsWith('/extensions/repositories')&&response.request().method()==='POST');
  await panel.getByRole('button',{name:'Ajouter le dépôt',exact:true}).click();
  assert.equal((await rejected).status(),400);
  await panel.getByRole('alert').waitFor();
  await panel.getByLabel('Adresse du dépôt',{exact:true}).fill(repositoryUrl);
  await panel.getByRole('button',{name:'Ajouter le dépôt',exact:true}).click();
  await panel.getByRole('heading',{name:'Repository browser fixture',exact:true}).waitFor();
  const versions=panel.getByLabel('Version de Repository browser fixture',{exact:true});
  assert.equal(await versions.inputValue(),'1.1.0');
  assert.equal(await panel.getByRole('button',{name:'Installer Repository browser fixture',exact:true}).isDisabled(),true);
  await versions.selectOption('1.0.0');
  await panel.getByText('Vérifier l’éditeur repository-browser',{exact:true}).click();
  await panel.getByRole('button',{name:'J’ai vérifié cette clé : approuver',exact:true}).click();
  await panel.getByText('Éditeur approuvé.',{exact:true}).waitFor();
  await page.screenshot({path:`${output}/catalogue-desktop.png`,fullPage:true});
  await panel.getByRole('button',{name:'Installer Repository browser fixture',exact:true}).click();
  await panel.getByText('Extension installée. Configurez ses paramètres et ses bibliothèques puis activez-la. Aucun redémarrage nécessaire.',{exact:true}).waitFor();
  const installed=await (await page.request.get(`${origin}/api/v1/admin/extensions`)).json();
  assert.equal(installed.extensions[0].manifest.version,'1.0.0');
  assert.equal(installed.extensions[0].enabled,false);
  assert.deepEqual(installed.extensions[0].libraryIds,[]);
  await page.reload();
  await panel.getByRole('button',{name:'Installées',exact:true}).click();
  await panel.getByRole('heading',{name:'Repository browser fixture',exact:true}).waitFor();
  await panel.getByRole('button',{name:'Dépôts',exact:true}).click();
  await panel.getByRole('heading',{name:'Repository browser test',exact:true}).waitFor();
  await panel.getByRole('button',{name:'Actualiser ce dépôt',exact:true}).click();
  await panel.getByText('Catalogue actualisé.',{exact:true}).waitFor();
  await panel.getByRole('button',{name:'Retirer le dépôt',exact:true}).click();
  await panel.getByText('Aucun dépôt enregistré.',{exact:true}).waitFor();
  await panel.getByRole('button',{name:'Installées',exact:true}).click();
  await panel.getByRole('heading',{name:'Repository browser fixture',exact:true}).waitFor();
  await page.setViewportSize({width:390,height:844});
  await page.evaluate(()=>window.scrollTo(0,0));
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
  await page.screenshot({path:`${output}/installed-mobile.png`,fullPage:true});
  assert.deepEqual(errors,[]);
  await writeFile(`${output}/report.json`,JSON.stringify({passed:true,browser:browser.version(),backend:'real Core / temporary SQLite',transport:'HTTPS documents injected only under cfg(test); no public repository claimed',checks:['private URL rejected','repository added and persisted','latest version selected','older version explicitly chosen','publisher trust required','signed package installed disabled','repository refresh and removal preserve installed extension','390px layout'],jsErrors:errors},null,2));
  console.log('Repository browser flow passed: add, catalogue, version selection, publisher trust, signed install, persistence, refresh/remove and mobile layout.');
} catch(error) {
  await page.screenshot({path:`${output}/failure.png`,fullPage:true});
  console.log(await page.locator('#extensions-section').innerText());
  throw error;
} finally {await browser.close();}

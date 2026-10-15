const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const {spawn} = require('node:child_process');
const {checkMedia} = require('./media-checks.cjs');
const {checkRemux} = require('./remux-checks.cjs');
const {checkRepresentative} = require('./representative-checks.cjs');
const scenario = process.env.MEDIA_TEST_SCENARIO || 'native';
const {createHash} = require('node:crypto');
const channel = process.env.MEDIA_BROWSER_CHANNEL || 'chrome';
const out = path.resolve(process.env.MEDIA_REPORT_DIR || `artifacts/browser-${channel}`);
fs.mkdirSync(out, {recursive:true});
const delay = ms => new Promise(resolve => setTimeout(resolve,ms));

async function firefoxAdapter() {
  const port = Number(process.env.MEDIA_WEBDRIVER_PORT || '4444');
  const base = `http://127.0.0.1:${port}`;
  const executable = process.env.MEDIA_GECKODRIVER || path.resolve('artifacts/tools/geckodriver/geckodriver.exe');
  const log = fs.openSync(path.join(out,'geckodriver.log'),'w');
  const child = spawn(executable,['--host','127.0.0.1','--port',String(port)], {windowsHide:true,stdio:['ignore',log,log]});
  let spawnError; child.on('error',error => { spawnError=error; });
  const call = async (method, route, body) => {
    const response = await fetch(base+route, {method, headers:{'Content-Type':'application/json'}, body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(45000)});
    const data = await response.json();
    if (!response.ok || data.value?.error) throw new Error(JSON.stringify(data.value));
    return data.value;
  };
  let session;
  const close = async () => {
    if(session) await call('DELETE',`/session/${session}`).catch(()=>{});
    child.kill(); fs.closeSync(log);
  };
  try {
    let ready=false;
    for(let i=0;i<50;i++) {
      if(spawnError) throw spawnError;
      if(child.exitCode !== null) throw new Error(`geckodriver exited ${child.exitCode}`);
      try { await call('GET','/status'); ready=true; break; } catch { await delay(100); }
    }
    if(!ready) throw new Error('geckodriver did not become ready');
    const info=await call('POST','/session',{capabilities:{alwaysMatch:{browserName:'firefox','moz:firefoxOptions':{binary:process.env.MEDIA_BROWSER_EXECUTABLE || 'C:\\Program Files\\Mozilla Firefox\\firefox.exe',args:['-headless']}}}});
    session=info.sessionId;
    await call('POST',`/session/${session}/timeouts`,{script:45000,pageLoad:20000});
    const evaluate = async (fn,arg) => {
      const result=await call('POST',`/session/${session}/execute/async`,{script:`const done=arguments[arguments.length-1]; Promise.resolve((${fn.toString()})(arguments[0])).then(value=>done({ok:true,value}),error=>done({ok:false,error:String(error)}));`,args:[arg??null]});
      if(!result.ok) throw new Error(result.error);
      return result.value;
    };
    const element = async selector => (await call('POST',`/session/${session}/element`,{using:'css selector',value:selector}))['element-6066-11e4-a52e-4f735466cecf'];
    return {version:info.capabilities.browserVersion, driver:info.capabilities['moz:geckodriverVersion'], pid:info.capabilities['moz:processID'] ?? null,
      evaluate, close, goto:url=>call('POST',`/session/${session}/url`,{url}),
      press:key=>{
        const value={Space:'\uE00D',Escape:'\uE00C',ArrowLeft:'\uE012',ArrowUp:'\uE013',ArrowRight:'\uE014',ArrowDown:'\uE015',Tab:'\uE004',Enter:'\uE007'}[key]||key;
        return call('POST',`/session/${session}/actions`,{actions:[{type:'key',id:'keyboard',actions:[{type:'keyDown',value},{type:'keyUp',value}]}]});
      },
      select: value=>evaluate(value=>{ const s=document.querySelector('#asset'); s.value=value; s.dispatchEvent(new Event('change',{bubbles:true})); },value),
      click:async(selector='#load')=>call('POST',`/session/${session}/element/${await element(selector)}/click`,{}),
      screenshot:async name=>fs.writeFileSync(path.join(out,name),Buffer.from(await call('GET',`/session/${session}/screenshot`),'base64'))};
  } catch(error) { await close(); throw error; }
}

async function playwrightAdapter() {
  const {chromium} = require('playwright');
  const options={headless:true};
  if(process.env.MEDIA_BROWSER_EXECUTABLE) options.executablePath=process.env.MEDIA_BROWSER_EXECUTABLE;
  else options.channel=channel;
  const browser=await chromium.launch(options);
  let pid=null,pidError;
  let session;
  try {
    session=await browser.newBrowserCDPSession();
    const info=await session.send('SystemInfo.getProcessInfo');
    pid=info.processInfo.find(process=>process.type==='browser')?.id ?? null;
  } catch(error) {pidError=String(error);}
  finally {if(session)await session.detach().catch(()=>{});}
  const page=await browser.newPage();
  const responses=[];
  page.on('response',r=>{if(r.url().includes('/content')) responses.push({url:new URL(r.url()).pathname,status:r.status(),headers:r.headers()});});
  return {version:await browser.version(), driver:`Playwright ${require('playwright/package.json').version}`, pid,pidError,responses,
    evaluate:(fn,arg)=>page.evaluate(fn,arg), goto:url=>page.goto(url), select:value=>page.selectOption('#asset',value), click:(selector='#load')=>page.click(selector),
    screenshot:name=>page.screenshot({path:path.join(out,name)}), close:()=>browser.close()};
}

function hashes() {
  const root=process.env.MEDIA_FIXTURE_DIR || 'tests/fixtures/generated';
  const names=['av1-opus.mkv','av1-opus.mp4','av1-opus.webm','av1-opus-fragmented.mp4'];
  for(const container of ['mkv','mp4']) for(const language of ['fr','en']) {const n=`language-remux/av1-opus-${container}-${language}.${container}`;if(fs.existsSync(path.join(root,n))) names.push(n);}
  if(scenario==='representative') names.push(...['representative.mkv','representative.mp4','representative-en.mkv','representative-en.mp4'].map(n=>`representative/${n}`));
  return Object.fromEntries(names.map(name=>[name,createHash('sha256').update(fs.readFileSync(path.join(root,name))).digest('hex')]));
}

async function main(){
  if(!['native','remux','representative'].includes(scenario)) throw new Error('MEDIA_TEST_SCENARIO must be native, remux or representative');
  const report={date:new Date().toISOString(),channel,headless:true,os:{platform:os.platform(),release:os.release(),arch:os.arch()},
    scenario,
    settings:'default browser features; no experimental audioTracks flags/preferences', hashesBefore:hashes(), results:[],
    limits:'Decoded pixels and PCM only; human listening and physical display NT. Remux files are played by native URL; no MSE append.'};
  let browser;
  try {
    browser=await (channel==='firefox'?firefoxAdapter():playwrightAdapter());
    report.browser=browser.version; report.driver=browser.driver;
    await browser.goto(process.env.MEDIA_TEST_URL || 'http://127.0.0.1:3000');
    const end=Date.now()+10000;
    while(!await browser.evaluate(()=>['fixture-mkv','fixture-mp4','fixture-webm','fixture-fmp4'].every(id=>Array.from(document.querySelector('#asset')?.options||[]).some(o=>o.value===id)))) {
      if(Date.now()>end) throw new Error('Inventory UI timeout'); await delay(100);
    }
    report.userAgent=await browser.evaluate(()=>navigator.userAgent);
    const assets=await browser.evaluate(()=>fetch('/api/v1/assets').then(r=>r.json()));
    const cases = scenario==='representative' ? assets.filter(a=>a.representative) : scenario==='remux' ? assets.filter(a=>['fixture-mkv','fixture-mp4'].includes(a.id)) : assets.filter(a=>!a.language_remux && !a.representative);
    if(cases.length!==(scenario==='remux'?2:4)) throw new Error('Incomplete scenario inventory');
    for(const asset of cases) {
      await browser.select(asset.id); await browser.click();
      const result=await browser.evaluate(scenario==='representative'?checkRepresentative:scenario==='remux'?checkRemux:checkMedia,asset);
      report.results.push(result);
      await browser.screenshot(`${asset.id}.png`);
    }
    report.missing=await browser.evaluate(async()=>{
      const response=await fetch('/api/v1/assets/missing/content');
      const v=document.querySelector('video');
      v.src='/api/v1/assets/missing/content';v.load();
      await new Promise(resolve=>{const finish=()=>{v.removeEventListener('error',finish);clearTimeout(timer);resolve();}; const timer=setTimeout(finish,5000);v.addEventListener('error',finish);});
      return {httpStatus:response.status,code:v.error?.code,message:v.error?.message,displayed:document.querySelector('#status').textContent};
    });
    report.responses=browser.responses || null;
    if(report.results.some(r=>!r.passed || r.audioSelection?.status==='failed') || report.missing.httpStatus!==404 || report.missing.code!==4) process.exitCode=1;
  } catch(error) {report.runnerFailure=error.message;process.exitCode=1;}
  finally {
    if(browser) await browser.close();
    report.hashesAfter=hashes();
    report.originalsUnchanged=JSON.stringify(report.hashesBefore)===JSON.stringify(report.hashesAfter);
    if(!report.originalsUnchanged) process.exitCode=1;
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));
    console.log(JSON.stringify({channel,browser:report.browser,scenario,runnerFailure:report.runnerFailure,results:report.results.map(r=>({id:r.id,passed:r.passed,failure:r.failure,audio:r.initialAudio,selection:r.audioSelection,channelGraphMatchesExpected:r.channelGraphMatchesExpected,nativeSubtitles:r.nativeTextTracks?.length,markerSkews:r.markerChecks?.map(m=>m.audioVideoMediaClockSkewSeconds),switches:r.switches?.map(s=>({language:s.language,passed:s.passed,audioMs:s.requestToStableAudioMs,videoMs:s.requestToRestoredVideoFrameMs,frameGapMs:s.crossSourceFrameGapMs,silenceMs:s.sampledSilenceTotalMs}))})),missing:report.missing,originalsUnchanged:report.originalsUnchanged},null,2));
  }
}
module.exports={firefoxAdapter,playwrightAdapter,main};
if(require.main===module) main().catch(error=>{console.error(error);process.exitCode=1;});

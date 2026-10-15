const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {createHash}=require('node:crypto');
const {firefoxAdapter,playwrightAdapter}=require('./browser-matrix.cjs');
const {checkMse}=require('./mse-checks.cjs');
const out=path.resolve(process.env.MEDIA_REPORT_DIR||'artifacts/mse');fs.mkdirSync(out,{recursive:true});
const root=path.resolve('tests/fixtures/generated');
const names=['representative.mkv','representative.mp4','representative-en.mkv','representative-en.mp4'].map(n=>'representative/'+n);
const hashes=()=>Object.fromEntries(names.map(n=>[n,createHash('sha256').update(fs.readFileSync(path.join(root,n))).digest('hex')]));
(async()=>{
  const r={date:new Date().toISOString(),channel:process.env.MEDIA_BROWSER_CHANNEL||'chrome',os:os.release(),hashesBefore:hashes(),resourceMeasurement:'NT: browser process tree CPU/RSS not instrumented',scope:'MSE minimal; no DASH engine; Video.js and Shaka NT'};
  let browser;
  try{browser=await(r.channel==='firefox'?firefoxAdapter():playwrightAdapter());r.browser=browser.version;r.driver=browser.driver;await browser.goto('http://127.0.0.1:3000/mse');await browser.click('#start');r.result=await browser.evaluate(checkMse);for(let i=0;i<3;i++){await browser.evaluate(i=>{for(let j=0;j<document.querySelector('video').textTracks.length;j++)document.querySelector('video').textTracks[j].mode=j===i?'showing':'disabled';},i);await new Promise(resolve=>setTimeout(resolve,120));await browser.screenshot('subtitle-'+i+'.png');}if(!r.result.passed)process.exitCode=1;}
  catch(error){r.failure=String(error);if(browser)r.progress=await browser.evaluate(()=>window.mseCheckProgress).catch(()=>null);process.exitCode=1;}
  finally{if(browser)await browser.close();r.hashesAfter=hashes();r.originalsUnchanged=JSON.stringify(r.hashesBefore)===JSON.stringify(r.hashesAfter);if(!r.originalsUnchanged)process.exitCode=1;fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(r,null,2));console.log(JSON.stringify({channel:r.channel,browser:r.browser,passed:r.result?.passed,failure:r.result?.failure||r.failure,switches:r.result?.switches.map(s=>({language:s.language,mode:s.mode,ms:s.requestToSignalMs})),originalsUnchanged:r.originalsUnchanged}));}
})().catch(error=>{console.error(error);process.exitCode=1;});

const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
function stats(values){const a=values.filter(Number.isFinite).sort((a,b)=>a-b);if(!a.length)return{n:0};return{n:a.length,median:a.length%2?a[(a.length-1)/2]:(a[a.length/2-1]+a[a.length/2])/2,p95NearestRank:a[Math.ceil(.95*a.length)-1],max:a.at(-1),min:a[0]};}
function resources(rows){
  const samples=rows.filter(r=>r.type==='sample');if(samples.length<2)return{status:'insufficient samples'};
  const duration=(samples.at(-1).elapsed_ms-samples[0].elapsed_ms)/1000,result={status:'measured',samples:samples.length,durationSeconds:duration,logicalProcessors:os.cpus().length,collectionMs:stats(samples.map(r=>r.collection_ms)),limitations:rows.find(r=>r.type==='metadata')?.limits};
  for(const group of ['browser','server']){
    const previous=new Map();let cpu=0;
    for(const row of samples)for(const p of row[group].processes){const key=p.process_id+':'+p.creation_utc;if(previous.has(key))cpu+=Math.max(0,p.cpu_seconds-previous.get(key));previous.set(key,p.cpu_seconds);}
    result[group]={observedCpuSeconds:cpu,meanCpuPercentOfOneLogicalCore:100*cpu/duration,meanCpuPercentOfMachine:100*cpu/duration/os.cpus().length,residentSumMiB:stats(samples.map(r=>r[group].sum_working_set_bytes/1024/1024)),distinctProcessIdentities:previous.size,rootPresentInEverySample:samples.every(r=>r[group].root_present)};
  }
  result.observerCpuSeconds=samples.at(-1).observer.cpu_seconds-samples[0].observer.cpu_seconds;
  return result;
}
function summarize(report,rows=[]){
  const r=report.result||{},active=(r.switches||[]).filter(s=>s.mode!=='paused'),paused=(r.switches||[]).filter(s=>s.mode==='paused');
  return{engine:report.engine,channel:report.channel,browser:report.browser,passed:r.passed,failure:report.failure,originalsUnchanged:report.originalsUnchanged,
    activeLatencyMs:stats(active.map(s=>s.requestToSignalMs)),pausedLatencyIncludingHoldAndResumeMs:stats(paused.map(s=>s.requestToSignalMs)),resumeToSignalMs:stats(paused.map(s=>s.resumeToSignalMs)),
    activeSampledSilenceTotalMs:stats(active.map(s=>s.sampledSilenceTotalMs)),activeMaxSampledSilenceRunMs:stats(active.map(s=>s.maxSampledSilenceRunMs)),activeMaxVideoCallbackGapMs:stats(active.map(s=>s.maxVideoCallbackGapMs)),baselineVideoGapMs:r.baselineVideoGapMs,
    activeSwitchesWithWaiting:active.filter(s=>s.events.some(e=>e.event==='waiting')).length,activeSwitchesWithSeeking:active.filter(s=>s.events.some(e=>e.event==='seeking')).length,
    allVideoSourcesUnchanged:(r.switches||[]).every(s=>s.sourceUnchanged),allActiveSwitchesHaveVideoProgress:active.every(s=>s.videoContinuityObserved),allManualSubtitleChoicesRetained:(r.switches||[]).every(s=>s.manualSubtitlePreserved),
    markerSkewMs:stats((r.markers||[]).map(m=>m.mediaClockSkewSeconds*1000)),maxPollMs:stats((r.switches||[]).map(s=>s.maxPollMs)),resources:resources(rows)};
}
if(require.main===module){const root=process.argv[2]||'artifacts/player-comparison-final';const results=fs.readdirSync(root).filter(n=>fs.existsSync(path.join(root,n,'report.json'))).map(n=>{const dir=path.join(root,n),report=JSON.parse(fs.readFileSync(path.join(dir,'report.json')));const p=path.join(dir,'resources.jsonl');return summarize(report,fs.existsSync(p)?fs.readFileSync(p,'utf8').trim().split('\n').map(JSON.parse):[]);});fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify({p95Method:'nearest rank; n12 active switches per engine/browser, descriptive only; includes collection observer overhead',results},null,2));console.log(JSON.stringify(results.map(r=>({engine:r.engine,channel:r.channel,passed:r.passed,failure:r.failure,active:r.activeLatencyMs,silence:r.activeSampledSilenceTotalMs,gap:r.activeMaxVideoCallbackGapMs,sync:r.markerSkewMs,resources:r.resources.status})),null,2));}
module.exports={stats,resources,summarize};

// Recompute derived statistics without rewriting immutable raw evidence.
const fs=require('node:fs'),path=require('node:path'),{createHash}=require('node:crypto');
function stats(values){const a=values.filter(Number.isFinite).sort((x,y)=>x-y);if(!a.length)return null;const m=Math.floor(a.length/2);return {n:a.length,min:a[0],median:a.length%2?a[m]:(a[m-1]+a[m])/2,p95:a[Math.ceil(a.length*.95)-1],p99:a.length>=100?a[Math.ceil(a.length*.99)-1]:null,max:a.at(-1)};}
// Quantiles over every attempted opening, not just the completed trajectories.
// A missing observation remains an explicit failure/deadline outcome, never 0ms.
function latencyOutcomes(trials){
  const metrics={};
  for(const [name,field]of Object.entries({frame:'firstFrameMs',rawPcm:'firstRawPcmMs',alignedPcm:'firstAudioMs'})){
    const observations=trials.map(t=>{
      const history=t.observerHistory?.find(h=>h.kind==='open');
      const sample=t.open?.trial||history||(t.diagnostic?.trial?.kind==='open'?t.diagnostic.trial:null);
      return sample?.[field];
    });
    const observed=observations.filter(Number.isFinite).sort((a,b)=>a-b);
    const percentile=p=>{if(trials.length<100)return {status:'insufficient-attempts',attempts:trials.length,minimum:100};const rank=Math.ceil(trials.length*p);return rank<=observed.length?{status:'observed',ms:observed[rank-1],rank}:{status:'unobserved-failure-or-deadline',rank};};
    metrics[name]={attempts:trials.length,observed:observed.length,unobserved:trials.length-observed.length,observedOnly:stats(observed),p95AllAttempts:percentile(.95),p99AllAttempts:percentile(.99)};
  }
  return {attempts:trials.length,trajectoryFailures:trials.filter(t=>t.error||t.diagnostic).length,timeouts:trials.filter(t=>t.timeout||/timeout/i.test(t.error||'')).length,metrics};
}
function summarize(raw){
  const result={method:{median:'Arithmetic mean of the two central values for even n',percentiles:'Nearest rank; p99 only if at least 100 observations in this population',failures:'Never assigned a fabricated finite latency; all raw failures remain in source evidence'},populations:{},http:[]};
  for(const key of new Set(raw.cells.map(c=>`${c.sessions}/${c.phase}`))){
    const cells=raw.cells.filter(c=>`${c.sessions}/${c.phase}`===key),trials=cells.flatMap(c=>c.trials),opens=trials.filter(t=>t.open),actions=trials.flatMap(t=>t.actions||[]);
    result.populations[key]={openingOutcomes:latencyOutcomes(trials),rawPcmMs:stats(trials.map(t=>(t.open?.trial||(t.diagnostic?.trial?.kind==='open'?t.diagnostic.trial:null))?.firstRawPcmMs)),initialSignedDriftSeconds:stats(trials.map(t=>(t.open?.trial||(t.diagnostic?.trial?.kind==='open'?t.diagnostic.trial:null))?.initialPcm?.signedDriftSeconds)),runs:cells.length,sessionAttempts:cells.reduce((n,c)=>n+c.sessions,0),observedCompletedOpenings:opens.length,failedCells:cells.filter(c=>!c.passed).length,failedTrajectories:trials.filter(t=>t.error||t.diagnostic).length,historicalLostOpeningMeasurements:trials.filter(t=>!t.open&&t.diagnostic?.trial?.kind!=='open').length,frameMs:stats(opens.map(t=>t.open.trial.firstFrameMs)),pcmMs:stats(opens.map(t=>t.open.trial.firstAudioMs)),switchPcmMs:stats(actions.filter(a=>a.trial.kind==='switch').map(a=>a.trial.firstAudioMs)),seekFrameMs:stats(actions.filter(a=>a.trial.kind==='seek').map(a=>a.trial.firstFrameMs)),seekPcmMs:stats(actions.filter(a=>a.trial.kind==='seek').map(a=>a.trial.firstAudioMs)),preparedReopenFrameMs:stats(actions.filter(a=>a.trial.kind==='reopen').map(a=>a.trial.firstRestoredFrameMs)),preparedReopenPcmMs:stats(actions.filter(a=>a.trial.kind==='reopen').map(a=>a.trial.firstAudioMs))};
  }
  for(const endpoint of raw.http||[]){const successes=endpoint.samples.filter(s=>s.status===(endpoint.component==='video'?200:206));result.http.push({component:endpoint.component,path:endpoint.path,attempts:endpoint.samples.length,failures:endpoint.samples.length-successes.length,headersMs:stats(successes.map(s=>s.headersMs)),through64KiBMs:stats(successes.map(s=>s.totalMs))});}
  return result;
}
if(require.main===module){const input=process.argv[2],output=process.argv[3];if(!input||!output)throw new Error('Usage: node streaming-report-summary.cjs raw.json derived.json');if(path.resolve(input)===path.resolve(output))throw new Error('Raw evidence must not be overwritten');const bytes=fs.readFileSync(input),result={sourceReport:path.basename(input),sourceSha256:createHash('sha256').update(bytes).digest('hex'),...summarize(JSON.parse(bytes))};fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({output,populations:Object.keys(result.populations).length}));}
module.exports={stats,summarize,latencyOutcomes};

const fs=require('node:fs'),path=require('node:path');
const {stats}=require('./summarize-player-comparison.cjs');
function summarizeTrials(rows){
  const groups=new Map();
  for(const row of rows.filter(r=>r.type==='trial')){
    const key=[row.channel,row.margin,row.mode].join('/');
    if(!groups.has(key))groups.set(key,[]);groups.get(key).push(row);
  }
  const result=[];
  const measure=trials=>{
    const ok=trials.filter(t=>t.passed),sw=ok.map(t=>t.switch);
    return{attempts:trials.length,completed:ok.length,failed:trials.filter(t=>!t.passed).map(t=>({round:t.round,from:t.from,failure:t.failure,phase:t.stage||(t.partial?.switches?.at(-1)?.passed===false?t.partial.switches.at(-1).mode:'unknown-before-target'),lastObservedSwitchMargin:t.partial?.switches?.at(-1)?.safeMargin??null})),
      latencyMs:stats(sw.map(s=>s.requestToSignalMs)),resumeLatencyMs:stats(sw.map(s=>s.resumeToSignalMs)),
      sampledSilenceTotalMs:stats(sw.map(s=>s.sampledSilenceTotalMs)),maxSampledSilenceRunMs:stats(sw.map(s=>s.maxSampledSilenceRunMs)),
      videoCallbackGapMs:stats(sw.map(s=>s.maxVideoCallbackGapMs)),droppedVideoFrames:stats(sw.filter(s=>s.videoQuality).map(s=>s.videoQuality.droppedFramesDelta)),
      droppedVideoFramesSum:sw.reduce((sum,s)=>sum+(s.videoQuality?.droppedFramesDelta||0),0),totalVideoFramesSum:sw.reduce((sum,s)=>sum+(s.videoQuality?.totalFramesDelta||0),0),
      callbackPresentedCounterSkips:stats(sw.map(s=>s.callbackPresentedCounterSkips)),
      waitingTrials:sw.filter(s=>s.events.some(e=>e.event==='waiting')).length,seekingTrials:sw.filter(s=>s.events.some(e=>e.event==='seeking')).length,
      maxPollMs:stats(sw.map(s=>s.maxPollMs)),sourceUnchanged:sw.every(s=>s.sourceUnchanged),pausePreserved:sw.filter(s=>s.mode==='paused').every(s=>s.pausePreserved),
      avSkewMs:stats(ok.map(t=>t.marker.mediaClockSkewSeconds*1000)),
      backwardSeekRegressions:ok.filter(t=>!t.backwardSeek.matchesSelectedLanguage).length,backwardSeekReplaysPreviousLanguage:ok.filter(t=>t.backwardSeek.previousSignal&&!t.backwardSeek.expectedSignal).length,
      audioBufferedSecondsBefore:stats(sw.filter(s=>s.buffersBefore?.audio).map(s=>s.buffersBefore.audio.reduce((sum,b)=>sum+b.end-b.start,0))),
      audioBufferedSecondsAfter:stats(sw.filter(s=>s.buffersAfter?.audio).map(s=>s.buffersAfter.audio.reduce((sum,b)=>sum+b.end-b.start,0)))};
  };
  for(const trials of groups.values())result.push({channel:trials[0].channel,margin:trials[0].margin,mode:trials[0].mode,...measure(trials),byDirection:{frToEn:measure(trials.filter(t=>t.from==='fr')),enToFr:measure(trials.filter(t=>t.from==='en'))}});
  return result.sort((a,b)=>a.channel.localeCompare(b.channel)||a.mode.localeCompare(b.mode)||a.margin-b.margin);
}
if(require.main===module){const root=process.argv[2]||'artifacts/safe-margin-final';const rows=fs.readdirSync(root).filter(n=>fs.existsSync(path.join(root,n,'trials.jsonl'))).flatMap(n=>fs.readFileSync(path.join(root,n,'trials.jsonl'),'utf8').trim().split('\n').map(JSON.parse));const result={method:'Medians and nearest-rank P95; active n20 (10/direction), pause/after-seek n4 each; profiles and failure counts retained; no resource sampler; 500ms post-signal',metadata:rows.filter(r=>r.type==='metadata'),ends:rows.filter(r=>r.type==='end'),runnerFailures:rows.filter(r=>r.type==='runner_failure'),results:summarizeTrials(rows)};fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result.results.filter(r=>r.mode==='playing').map(r=>({browser:r.channel,margin:r.margin,n:r.attempts,ok:r.completed,median:r.latencyMs.median,p95:r.latencyMs.p95NearestRank,silence:r.sampledSilenceTotalMs,waiting:r.waitingTrials,seekRegressions:r.backwardSeekRegressions})),null,2));}
module.exports={summarizeTrials};

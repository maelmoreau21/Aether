const assert=require('node:assert/strict');
const {summarizeTrials}=require('./summarize-safe-margin.cjs');
const trial=(from,passed)=>({type:'trial',channel:'chrome',margin:.5,mode:'playing',round:1,from,passed,failure:passed?undefined:'timeout retained',
  switch:{requestToSignalMs:100,resumeToSignalMs:null,sampledSilenceTotalMs:0,maxSampledSilenceRunMs:0,maxVideoCallbackGapMs:50,videoQuality:{droppedFramesDelta:2,totalFramesDelta:20},callbackPresentedCounterSkips:1,events:[],maxPollMs:12,sourceUnchanged:true},
  marker:{mediaClockSkewSeconds:.01},backwardSeek:{matchesSelectedLanguage:false,previousSignal:true,expectedSignal:false}});
const [r]=summarizeTrials([trial('fr',true),trial('en',true),trial('en',false)]);
assert.equal(r.attempts,3);assert.equal(r.completed,2);assert.equal(r.failed.length,1);
assert.equal(r.failed[0].failure,'timeout retained');
assert.equal(r.droppedVideoFramesSum,4);assert.equal(r.totalVideoFramesSum,40);
assert.equal(r.backwardSeekRegressions,2);assert.equal(r.backwardSeekReplaysPreviousLanguage,2);
assert.equal(r.byDirection.frToEn.attempts,1);assert.equal(r.byDirection.enToFr.attempts,2);
assert.equal(r.latencyMs.n,2);assert.equal(r.resumeLatencyMs.n,0);
console.log('Safe-margin statistics: failures, directions, seek regressions and frame deltas passed');

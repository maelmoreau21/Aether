const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm');
const {latencyOutcomes}=require('./streaming-report-summary.cjs');
const {installStreamingObserver}=require('./streaming-system-observer.cjs');
test('failure-inclusive p99 preserves two missing observations',()=>{
 const trials=Array.from({length:100},(_,i)=>i<98?{open:{trial:{firstFrameMs:i+1}}}:{error:'Timeout',timeout:true});
 const result=latencyOutcomes(trials);
 assert.equal(result.metrics.frame.p95AllAttempts.ms,95);
 assert.equal(result.metrics.frame.p99AllAttempts.status,'unobserved-failure-or-deadline');
 assert.equal(result.timeouts,2);
 assert.equal(latencyOutcomes(trials.slice(0,99)).metrics.frame.p99AllAttempts.status,'insufficient-attempts');
});
test('PCM raw is recorded despite paused video and initial one-second drift',()=>{
 let poll,now=0;
 const audio={paused:false,muted:false,currentTime:1,dataset:{internalAudioTrack:'internal-1'}};
 const video={paused:true,seeking:false,currentTime:0,currentSrc:'blob:fixture'};
 const document={querySelector:q=>q.includes('video')?video:null,querySelectorAll:q=>q.includes('audio')?[audio]:[],addEventListener(){}};
 class AudioContext{constructor(){this.sampleRate=48000;this.destination={};}resume(){} createMediaElementSource(){return {connect(){},disconnect(){}};}createAnalyser(){return {fftSize:2048,connect(){},getFloatFrequencyData(b){b.fill(-100);b[25]=-10;},getFloatTimeDomainData(b){b.fill(.1);}};}}
 const sandbox={document,AudioContext,window:{},performance:{now:()=>now,timeOrigin:1000},MutationObserver:class{observe(){}},setInterval:f=>{poll=f;}};
 vm.runInNewContext(`(${installStreamingObserver.toString()})()`,sandbox);
 sandbox.window.__streamingSystem.begin('open');now=8;poll();
 const trial=sandbox.window.__streamingSystem.trials[0];
 assert.equal(trial.firstRawPcmMs,8);assert.equal(trial.initialPcm.signedDriftSeconds,1);assert.equal(trial.firstAudioMs,undefined);
 video.paused=false;video.currentTime=1;now=16;poll();
 assert.equal(trial.firstAudioMs,16);assert.equal(trial.firstRawPcmMs,8);
});

test('transparent network cut closes real sockets and permits explicit restoration',async()=>{
 const net=require('node:net'),{once}=require('node:events'),{createTcpProfile}=require('./streaming-tcp-profile.cjs');
 const upstream=net.createServer(s=>s.pipe(s));upstream.listen(0,'127.0.0.1');await once(upstream,'listening');
 const proxy=createTcpProfile({targetPort:upstream.address().port});const port=await proxy.start();
 try{
  const first=net.connect(port,'127.0.0.1');await once(first,'connect');first.write('echo');assert.equal((await once(first,'data'))[0].toString(),'echo');
  const closed=once(first,'close');proxy.setDisconnected(true);await closed;
  const blocked=net.connect(port,'127.0.0.1');blocked.on('error',()=>{});await once(blocked,'close');
  proxy.setDisconnected(false);const restored=net.connect(port,'127.0.0.1');await once(restored,'connect');restored.write('restored');assert.equal((await once(restored,'data'))[0].toString(),'restored');restored.destroy();
  assert.equal(proxy.snapshot().networkCuts,1);
 }finally{await proxy.close();await new Promise(r=>upstream.close(r));}
});

test('opening observations survive later failed seek and incomplete opening',()=>{
 const trials=[{error:'Seek Timeout',observerHistory:[{kind:'open',firstFrameMs:20,firstRawPcmMs:15,firstAudioMs:25},{kind:'seek'}],diagnostic:{trial:{kind:'seek'}}},{error:'Timeout',diagnostic:{trial:{kind:'open',firstFrameMs:30,firstRawPcmMs:12}}}];
 const result=latencyOutcomes(trials);
 assert.equal(result.metrics.frame.observed,2);assert.equal(result.metrics.rawPcm.observed,2);assert.equal(result.metrics.alignedPcm.observed,1);
 assert.equal(result.trajectoryFailures,2);
});

'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const {createAudioSeekPolicy}=require('../crates/core/src/audio-seek-policy.js');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

class VideoMock extends EventTarget {
  constructor(paused){super();this.position=3.25;this.paused=paused;this.playbackRate=1.5;this.readyState=3;this.seeking=false;this.error=null;this.playCalls=0;}
  get currentTime(){return this.position;}
  set currentTime(value){this.position=value;this.seeking=true;this.dispatchEvent(new Event('seeking'));queueMicrotask(()=>{this.seeking=false;this.dispatchEvent(new Event('seeked'));});}
  pause(){this.paused=true;}
  play(){this.playCalls++;this.paused=false;return Promise.resolve();}
}
class PlayerMock extends EventTarget {
  constructor(video,{append=true,move=.03}={}){super();this.video=video;this.append=append;this.move=move;this.calls=[];this.tracks=[{id:1,language:'fr',audioId:11,videoId:5,active:false},{id:2,language:'en',audioId:12,videoId:5,active:true}];this.ranges={audio:[],video:[]};}
  getVariantTracks(){return this.tracks;}
  getBufferedInfo(){return this.ranges;}
  selectVariantTrack(variant,clear,margin){
    this.calls.push({id:variant.id,clear,margin});
    this.tracks.forEach(track=>{track.active=track.id===variant.id;});
    this.video.playbackRate=1;
    // Shaka's flush can emit seeking while the recovery lock is already set.
    this.video.currentTime=this.video.currentTime+this.move;
    if(this.append)setTimeout(()=>{
      this.ranges={audio:[{start:0,end:12}],video:[{start:0,end:12}]};
      const event=new Event('segmentappended');Object.assign(event,{contentType:'audio',start:0,end:12});this.dispatchEvent(event);
    },5);
  }
}
function fixture({paused=true,timeoutMs=700,...playerOptions}={}){
  const video=new VideoMock(paused),player=new PlayerMock(video,playerOptions),states=[];
  const policy=createAudioSeekPolicy({video,player,timeoutMs,onState:state=>states.push(state)});
  const target=player.tracks[1];policy.noteSelection(target,1);
  return {video,player,policy,target,states};
}
async function finished(policy,timeout=1200){
  const end=performance.now()+timeout;
  while(!policy.history.length){if(performance.now()>end)throw new Error('Test recovery did not finish');await delay(5);}
  return policy.history.at(-1);
}

for(const paused of [true,false])test(`successful recovery preserves ${paused?'pause':'playback'}, position and rate`,async t=>{
  const f=fixture({paused});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  assert.equal(f.policy.state.phase,'recovering');
  assert.throws(()=>f.policy.assertSelectable(),/in progress/);
  assert.throws(()=>f.policy.noteSelection(f.target,0),/in progress/);
  assert.throws(()=>f.policy.setEnabled(false),/in progress/);
  const operation=await finished(f.policy);
  assert.equal(operation.success,true,operation.error);
  assert.equal(operation.attempts,1);assert.equal(operation.restores,1);
  assert.deepEqual(f.player.calls,[{id:2,clear:true,margin:0}]);
  assert.equal(f.video.currentTime,3.25);assert.equal(f.video.paused,paused);assert.equal(f.video.playbackRate,1.5);
  assert.equal(f.video.playCalls,paused?0:1);
  assert.ok(f.policy.state.ignoredInternalSeeks>=2);
  assert.equal(f.policy.state.phase,'idle');assert.equal(f.policy.dirty,false);
  assert.doesNotThrow(()=>f.policy.assertSelectable());
  // Further seeks after a complete purge do not recurse or attempt another purge.
  f.video.currentTime=2;await delay(25);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.history.length,1);
});

test('a concurrent different seek cancels once and blocks further selection',async t=>{
  const f=fixture({append:false,move:0,timeoutMs:100});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  f.video.currentTime=.5;
  const operation=await finished(f.policy);
  assert.equal(operation.success,false);assert.match(operation.error,/interrupted/);
  assert.equal(f.video.paused,true);assert.equal(f.video.currentTime,.5);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.state.phase,'error');
  assert.throws(()=>f.policy.assertSelectable(),/interrupted/);
  assert.throws(()=>f.policy.noteSelection(f.target,0),/interrupted/);
  f.video.currentTime=2;await delay(100);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.history.length,1);
});

test('missing fresh audio times out terminally without retry',async t=>{
  const f=fixture({append:false,move:0,timeoutMs:50});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  const operation=await finished(f.policy);
  assert.equal(operation.success,false);assert.match(operation.error,/timeout: fresh selected audio/);
  assert.equal(operation.attempts,1);assert.equal(f.video.paused,true);
  assert.equal(f.policy.state.phase,'error');
  assert.throws(()=>f.policy.assertSelectable(),/timeout/);
  assert.throws(()=>f.policy.noteSelection(f.target,4),/timeout/);
  for(const position of [1,2,4])f.video.currentTime=position;
  await delay(100);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.history.length,1);
});

test('margin zero on a clean policy never triggers recovery',async t=>{
  const video=new VideoMock(false),player=new PlayerMock(video);
  const policy=createAudioSeekPolicy({video,player,timeoutMs:50});t.after(()=>policy.dispose());
  policy.noteSelection(player.tracks[1],0);
  for(const position of [1,4,.5])video.currentTime=position;
  await delay(70);
  assert.equal(policy.dirty,false);assert.equal(policy.state.phase,'idle');assert.equal(policy.history.length,0);assert.equal(player.calls.length,0);
  assert.equal(video.paused,false);assert.equal(video.playbackRate,1.5);
});

test('same-position internal seek is ignored rather than starting another operation',async t=>{
  const f=fixture({move:0});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  f.video.dispatchEvent(new Event('seeking'));
  const operation=await finished(f.policy);
  assert.equal(operation.success,true,operation.error);assert.equal(operation.attempts,1);
  assert.equal(operation.restores,0);assert.ok(f.policy.state.ignoredInternalSeeks>=2);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.history.length,1);
});

test('watchdog makes a post-recovery playback stall terminal without retry',async t=>{
  const f=fixture({paused:false,move:0});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  const operation=await finished(f.policy);
  assert.equal(operation.success,true,operation.error);
  assert.equal(f.policy.state.phase,'idle');assert.equal(f.video.paused,false);
  // Real time: play() succeeded, but the mock media clock stops advancing.
  await delay(2300);
  assert.equal(f.policy.state.phase,'error');
  assert.match(f.policy.state.error,/stalled after audio recovery/);
  assert.equal(operation.success,false);assert.equal(f.video.paused,true);
  assert.throws(()=>f.policy.assertSelectable(),/stalled/);
  assert.throws(()=>f.policy.noteSelection(f.target,1),/stalled/);
  f.video.currentTime=1;await delay(30);
  assert.equal(f.player.calls.length,1);assert.equal(f.policy.history.length,1);
});

test('watchdog permits an intentional pause longer than its stall threshold',async t=>{
  const f=fixture({paused:true,move:0});t.after(()=>f.policy.dispose());
  f.video.currentTime=3.25;
  const operation=await finished(f.policy);
  assert.equal(operation.success,true,operation.error);
  await delay(2300);
  assert.equal(f.policy.state.phase,'idle');assert.equal(f.policy.state.error,null);
  assert.equal(operation.success,true);assert.equal(f.video.paused,true);
  assert.equal(f.video.playCalls,0);assert.equal(f.player.calls.length,1);
  assert.doesNotThrow(()=>f.policy.assertSelectable());
});

test('a real different-audio selection at margin zero clears dirty history without video purge',async t=>{
  const f=fixture();t.after(()=>f.policy.dispose());
  assert.equal(f.policy.dirty,true);
  f.policy.noteSelection(f.player.tracks[0],0);
  assert.equal(f.policy.dirty,false);
  f.video.currentTime=.5;await delay(70);
  assert.equal(f.policy.state.phase,'idle');assert.equal(f.policy.history.length,0);
  assert.equal(f.player.calls.length,0);
});

test('reselecting the same audio at margin zero keeps dirty history even with another variant ID',async t=>{
  const f=fixture({move:0});t.after(()=>f.policy.dispose());
  f.policy.noteSelection(f.target,0);
  assert.equal(f.policy.dirty,true);
  const sameAudioVariant={...f.target,id:3,active:false};f.player.tracks.push(sameAudioVariant);
  f.policy.noteSelection(sameAudioVariant,0);
  assert.equal(f.policy.dirty,true);
  f.video.currentTime=3.25;
  const operation=await finished(f.policy);
  assert.equal(operation.success,true,operation.error);
  assert.deepEqual(f.player.calls,[{id:3,clear:true,margin:0}]);
  assert.equal(f.policy.dirty,false);
});

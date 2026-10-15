const assert=require('node:assert/strict');
const {stats,resources}=require('./summarize-player-comparison.cjs');
assert.deepEqual(stats([4,1,3,2]),{n:4,median:2.5,p95NearestRank:4,max:4,min:1});
assert.equal(stats(Array.from({length:20},(_,i)=>i+1)).p95NearestRank,19);
// A disappearing child must not subtract CPU; a reused PID is a new identity.
const proc=(id,creation,cpu)=>({process_id:id,creation_utc:creation,cpu_seconds:cpu});
const row=(ms,processes)=>({type:'sample',elapsed_ms:ms,collection_ms:1,browser:{processes,sum_working_set_bytes:1048576,root_present:true},server:{processes:[],sum_working_set_bytes:0,root_present:true},observer:{cpu_seconds:ms/10000}});
const r=resources([row(0,[proc(1,'a',100),proc(2,'b',10)]),row(1000,[proc(1,'a',100.5)]),row(2000,[proc(1,'a',101),proc(2,'new',50)])]);
assert.equal(r.browser.observedCpuSeconds,1);
assert.equal(r.browser.meanCpuPercentOfOneLogicalCore,50);
assert.equal(r.browser.distinctProcessIdentities,3);
assert.equal(r.browser.residentSumMiB.max,1);
console.log('Statistics: median, nearest-rank P95 and CPU identity accounting passed');

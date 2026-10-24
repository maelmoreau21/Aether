import {describe,expect,it} from 'vitest';
import {EncodedVideoBuffer} from './encoded-video-buffer';
describe('encoded MSE buffer accounting',()=>{
  it('caps retained encoded bytes at 128 MiB without allocating that media in the test',()=>{
    const ledger=new EncodedVideoBuffer();
    for(let n=1;n<=2048;n++)ledger.append(65536,n*65536);
    expect(ledger.bytes).toBe(128*1024*1024);expect(()=>ledger.append(1,128*1024*1024+1)).toThrow('128 Mio');
  });
  it('releases only whole blocks before an actually evicted RAP and pins initialization conservatively',()=>{
    const ledger=new EncodedVideoBuffer();ledger.append(65536,65536);ledger.append(65536,131072);ledger.append(65536,196608);
    ledger.evictedBefore(150000);expect(ledger.bytes).toBe(131072);
    ledger.evictedBefore(196608);expect(ledger.bytes).toBe(65536);
    ledger.clear();expect(ledger.bytes).toBe(0);ledger.append(100,100);expect(ledger.bytes).toBe(100);
  });
  it('coalesces tiny delivery blocks and delays freeing a block that crosses the RAP boundary',()=>{
    const ledger=new EncodedVideoBuffer();ledger.append(65536,65536);
    ledger.append(100,65636);ledger.append(100,65736);
    ledger.evictedBefore(65700);expect(ledger.bytes).toBe(65736);
    ledger.evictedBefore(65736);expect(ledger.bytes).toBe(65536);
  });
});

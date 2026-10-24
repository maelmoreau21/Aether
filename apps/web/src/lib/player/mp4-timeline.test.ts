import {describe,expect,it} from 'vitest';
import {mp4TimelineOffset} from './mp4-timeline';
function box(type:string,...parts:Uint8Array[]) {
  const bytes=new Uint8Array(8+parts.reduce((sum,p)=>sum+p.length,0));
  new DataView(bytes.buffer).setUint32(0,bytes.length);bytes.set([...type].map(c=>c.charCodeAt(0)),4);
  let cursor=8;for(const part of parts){bytes.set(part,cursor);cursor+=part.length;}return bytes;
}
function fixture(offset:number,mediaTime=0,version=0) {
  const mvhd=new Uint8Array(100);new DataView(mvhd.buffer).setUint32(12,1000);
  const count=offset?2:1,stride=version?20:12,elst=new Uint8Array(8+count*stride),view=new DataView(elst.buffer);
  elst[0]=version;view.setUint32(4,count);
  for(let i=0;i<count;i++){
    const at=8+i*stride,duration=i===0&&offset?offset*1000:0,time=i===0&&offset?-1:mediaTime;
    if(version){view.setBigUint64(at,BigInt(duration));view.setBigInt64(at+8,BigInt(time));view.setInt32(at+16,65536);}
    else{view.setUint32(at,duration);view.setInt32(at+4,time);view.setInt32(at+8,65536);}
  }
  return box('moov',box('mvhd',mvhd),box('trak',box('edts',box('elst',elst)))).buffer;
}
describe('generated fragmented MP4 timeline',()=>{
  it('recovers an exact source keyframe offset from leading empty edits',()=>{
    expect(mp4TimelineOffset(fixture(0))).toBe(0);expect(mp4TimelineOffset(fixture(15))).toBe(15);
    expect(mp4TimelineOffset(fixture(900,0,1))).toBe(900);
  });
  it('rejects unsupported edit origins and malformed/truncated boxes instead of guessing',()=>{
    expect(()=>mp4TimelineOffset(fixture(15,10))).toThrow('Origine');
    expect(()=>mp4TimelineOffset(fixture(15).slice(0,-1))).toThrow();
    expect(()=>mp4TimelineOffset(new ArrayBuffer(7))).toThrow();
  });
});

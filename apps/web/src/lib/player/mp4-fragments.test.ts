import {describe,expect,it} from 'vitest';
import {Mp4Fragments,mp4Timescale,mp4RandomAccess} from './mp4-fragments';
export function box(type:string,...parts:Uint8Array[]) {
  const data=new Uint8Array(8+parts.reduce((n,p)=>n+p.length,0));new DataView(data.buffer).setUint32(0,data.length);data.set([...type].map(c=>c.charCodeAt(0)),4);
  let at=8;for(const part of parts){data.set(part,at);at+=part.length;}return data;
}
export function initialization() {
  const mvhd=new Uint8Array(100),mdhd=new Uint8Array(24);new DataView(mvhd.buffer).setUint32(12,1000);new DataView(mdhd.buffer).setUint32(12,12000);
  return box('moov',box('mvhd',mvhd),box('trak',box('mdia',box('mdhd',mdhd))));
}
function fragment(time:number,sampleFlags=0x02000000) {
  const tfhd=new Uint8Array(12),tfdt=new Uint8Array(12),trun=new Uint8Array(8);
  const header=new DataView(tfhd.buffer);header.setUint32(0,32);header.setUint32(4,1);header.setUint32(8,sampleFlags);
  tfdt[0]=1;new DataView(tfdt.buffer).setBigUint64(4,BigInt(time*12000));new DataView(trun.buffer).setUint32(4,1);
  return box('moof',box('traf',box('tfhd',tfhd),box('tfdt',tfdt),box('trun',trun)));
}
describe('bounded progressive MP4 index',()=>{
  it('recognizes keyframes and their decode times while retaining no mdat bytes',()=>{
    const found:{type:string;bytes:ArrayBuffer;start:number}[]=[];
    const parser=new Mp4Fragments((type,bytes,start)=>found.push({type,bytes,start}));
    const blocks=[initialization(),box('mdat',new Uint8Array(100)),fragment(15),box('mdat',new Uint8Array(20))];
    for(const block of blocks)for(const byte of block)parser.push(new Uint8Array([byte]));parser.finishStream();
    expect(found.map(b=>b.type)).toEqual(['moov','moof']);expect(mp4Timescale(found[0].bytes)).toBe(12000);expect(mp4RandomAccess(found[1].bytes,12000)).toBe(15);
    expect(found[0].start).toBe(0);expect(found[1].start).toBe(blocks[0].byteLength+blocks[1].byteLength);
    expect(mp4RandomAccess(fragment(16,0x01010000).buffer,12000)).toBeUndefined();
  });
  it('rejects oversized indexes and truncated streams before retaining movie data',()=>{
    const parser=new Mp4Fragments(()=>{}),header=new Uint8Array(8);new DataView(header.buffer).setUint32(0,2*1024*1024);header.set([... 'moov'].map(c=>c.charCodeAt(0)),4);
    expect(()=>parser.push(header)).toThrow('1 Mio');
    const partial=new Mp4Fragments(()=>{});partial.push(new Uint8Array([0,0,0]));expect(()=>partial.finishStream()).toThrow('tronqué');
  });
});

import {describe,it,expect} from 'vitest';
import {parseLyrics} from './lyrics';
describe('library lyrics',()=>{
  it('supports repeated timestamps, fractions, offset and sorting',()=>{
    const lines=parseLyrics('[ar:Artist]\n[offset:500]\n[00:12.20][00:02.200]Refrain\n[00:05]Next');
    expect(lines.map(line=>line.text)).toEqual(['Refrain','Next','Refrain']);
    [1.7,4.5,11.7].forEach((time,index)=>expect(lines[index].time).toBeCloseTo(time,6));
  });
  it('keeps plain lyrics as inert text and bounds timestamp offsets',()=>{
    expect(parseLyrics('<img src=x onerror=alert(1)>\nHello')).toEqual([{time:null,text:'<img src=x onerror=alert(1)>'},{time:null,text:'Hello'}]);
    expect(parseLyrics('[offset:99999999]\n[00:01]First')[0].time).toBe(0);
  });
});

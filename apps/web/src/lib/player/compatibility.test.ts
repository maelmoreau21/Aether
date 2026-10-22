import {describe,expect,it,vi} from 'vitest';
import {unsupportedAudio} from './compatibility';
import type {Source} from './types';
const dts:Source={id:'original',audioTrackId:'internal-default',language:'dut',url:'/signed?token=secret',mime:'video/x-matroska',tracks:[{index:0,codec_type:'video',codec_name:'h264'},{index:1,codec_type:'audio',codec_name:'dts',profile:'DTS-HD MA',channels:6}]};
describe('negative audio capability checks',()=>{
  it('identifies DTS-HD MA without exposing source URLs',()=>{
    const capability=vi.fn(()=> '');const error=unsupportedAudio(dts,capability);
    expect(error).toContain('DTS-HD MA');expect(error).not.toContain('secret');
    expect(capability).toHaveBeenCalledWith('video/x-matroska; codecs="dtsl"');
  });
  it('does not reject a platform that declares DTS support',()=>{
    expect(unsupportedAudio(dts,()=> 'probably')).toBeUndefined();
  });
  it('does not infer the default selected codec from a mixed original',()=>{
    const mixed={...dts,tracks:[...dts.tracks!,{index:2,codec_type:'audio',codec_name:'opus'}]};
    expect(unsupportedAudio(mixed,()=> '')).toBeUndefined();
    expect(unsupportedAudio({...mixed,audioTrackId:'internal-1'},()=> '')).toContain('DTS-HD MA');
    expect(unsupportedAudio({...mixed,audioTrackId:'internal-2'},()=> '')).toBeUndefined();
  });
  it('never rejects AV1/Opus on the basis of an incomplete codec string',()=>{
    const capability=vi.fn(()=> '');
    expect(unsupportedAudio({...dts,tracks:[{codec_type:'audio',codec_name:'opus'}]},capability)).toBeUndefined();
    expect(capability).not.toHaveBeenCalled();
  });
});

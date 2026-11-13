import {describe,expect,it,vi} from 'vitest';
import {mediaCompatibilityWarning,subtitleCompatibilityWarning,unsupportedAudio} from './compatibility';
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

describe('format compatibility warnings at usage time',()=>{
  const av1Opus:Source={
    id:'av1',
    audioTrackId:'internal-1',
    language:'fr',
    url:'/video',
    mime:'video/mp4',
    tracks:[
      {index:0,codec_type:'video',codec_name:'av1'},
      {index:1,codec_type:'audio',codec_name:'opus'}
    ]
  };

  it('warns when the browser does not declare AV1 support',()=>{
    const warning=mediaCompatibilityWarning(av1Opus,mime=>mime.includes('av01')?'':'probably');
    expect(warning).toMatch(/AV1/i);
  });

  it('warns when the browser does not declare Opus support on a direct stream',()=>{
    const warning=mediaCompatibilityWarning(av1Opus,mime=>mime.includes('opus')?'':'probably');
    expect(warning).toMatch(/Opus/i);
  });

  it('stays silent when AV1 and Opus are declared supported',()=>{
    expect(mediaCompatibilityWarning(av1Opus,()=>'probably')).toBeUndefined();
  });

  it('skips direct Opus warning when using prepared or external audio',()=>{
    expect(mediaCompatibilityWarning({...av1Opus,prepare:true},mime=>mime.includes('opus')?'':'probably')).toBeUndefined();
    expect(mediaCompatibilityWarning({...av1Opus,externalAudioUrl:'/audio.ogg'},mime=>mime.includes('opus')?'':'probably')).toBeUndefined();
  });

  it('warns on ASS/SSA subtitle tracks and stays silent on WebVTT/SRT',()=>{
    expect(subtitleCompatibilityWarning({id:'s1',language:'fr',label:'FR',url:'/s.vtt',assUrl:'/s.ass'})).toMatch(/ASS\/SSA/);
    expect(subtitleCompatibilityWarning({id:'s2',language:'fr',label:'FR',url:'/s.vtt',sourceFormat:'ssa'})).toMatch(/ASS\/SSA/);
    expect(subtitleCompatibilityWarning({id:'s3',language:'fr',label:'FR',url:'/s.vtt',sourceFormat:'srt'})).toBeUndefined();
    expect(subtitleCompatibilityWarning(null)).toBeUndefined();
  });
});

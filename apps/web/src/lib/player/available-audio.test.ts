import { describe, expect, it } from 'vitest';
import { audioChoices } from './available-audio';
import type { Media, Source } from './types';

const source = (id: string, audioTrackId = id): Source => ({id,audioTrackId,language:'fr',url:`/${id}`,mime:'video/mp4'});
const media = (sources: Source[]): Media => ({id:'film',title:'Film',sources,subtitles:[]});
describe('operational audio choices', () => {
  it('counts switchable sources rather than detected container tracks', () => {
    expect(audioChoices(media([{...source('one'),tracks:[{codec_type:'audio'},{codec_type:'audio'}]}]))).toHaveLength(1);
  });
  it('deduplicates the same audio exposed by alternate packaging', () => {
    expect(audioChoices(media([source('original','fr'),source('prepared','fr'),source('english','en')])).map(item=>item.audioTrackId)).toEqual(['fr','en']);
  });
  it('does not advertise disconnected or revoked sources', () => {
    expect(audioChoices(media([source('one'),{...source('two'),nodeStatus:'disconnected'},{...source('three'),nodeStatus:'revoked'}]))).toHaveLength(1);
    expect(audioChoices({...media([source('one')]),available:false})).toEqual([]);
  });
  it('collapses original and prepared forms of the same internal track while preserving the current choice', () => {
    const original = {...source('original','internal-default'),tracks:[{index:1,codec_type:'audio'}]};
    const prepared = {...source('prepared','internal-1'),prepare:true};
    expect(audioChoices(media([original,prepared]))).toEqual([original]);
    expect(audioChoices(media([original,prepared]),'internal-1')).toEqual([prepared]);
    expect(audioChoices(media([original,prepared,source('second','internal-2')]))).toHaveLength(2);
  });
});

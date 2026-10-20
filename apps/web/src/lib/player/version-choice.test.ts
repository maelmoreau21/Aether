import { describe,it,expect } from 'vitest';
import { bestVersion } from './version-choice';
import type { Media } from './types';
function version(id:string,width:number,height:number,codec='av1'):Media{return{id,title:id,available:true,sources:[{id,audioTrackId:id,language:'fr',url:'/'+id,mime:'video/mp4',tracks:[{codec_type:'video',codec_name:codec,width,height}]}],subtitles:[]};}
describe('automatic available version',()=>{
 it('chooses the highest compatible resolution and ignores disconnected Nodes',()=>{const hd=version('hd',1920,1080),uhd=version('uhd',3840,2160);expect(bestVersion([hd,uhd],()=> 'probably')?.id).toBe('uhd');uhd.nodeStatus='disconnected';expect(bestVersion([hd,uhd],()=> 'probably')?.id).toBe('hd');});
 it('excludes a codec the browser explicitly rejects',()=>{const hd=version('hd',1920,1080),uhd=version('uhd',3840,2160,'hevc');expect(bestVersion([uhd,hd],mime=>mime.includes('hvc1')?'':'probably')?.id).toBe('hd');});
 it('returns no declared compatible source instead of claiming direct play',()=>{expect(bestVersion([version('hd',1920,1080)],()=> '')).toBeUndefined();});
});

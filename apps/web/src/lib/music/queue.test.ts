import {get} from 'svelte/store';
import {afterEach,describe,it,expect} from 'vitest';
import {musicQueue,playMusic,nextMusic,removeMusic,shuffleMusic,clearMusic,enqueueMusic} from './queue';
const tracks=Array.from({length:3},(_,i)=>({id:String(i),title:`Track ${i}`,artist:'Artist',album:'Album',mime:'audio/ogg',codec:'opus',duration:12}));
afterEach(clearMusic);
describe('music queue',()=>{
  it('advances and stops at the end unless repetition is requested',()=>{
    playMusic(tracks,1);nextMusic();expect(get(musicQueue).index).toBe(2);nextMusic(true);expect(get(musicQueue).playing).toBe(false);
    musicQueue.update(q=>({...q,repeat:'all'}));nextMusic(true);expect(get(musicQueue).index).toBe(0);expect(get(musicQueue).playing).toBe(true);
  });
  it('preserves the current title during shuffle and removal before it',()=>{
    playMusic(tracks,1);removeMusic(0);expect(get(musicQueue).tracks[get(musicQueue).index].id).toBe('1');shuffleMusic();expect(get(musicQueue).tracks[0].id).toBe('1');
    expect(new Set(get(musicQueue).tracks.map(t=>t.id)).size).toBe(2);
  });
  it('bounds the queue and clears private titles on session teardown',()=>{
    playMusic(Array.from({length:1000},(_,i)=>({...tracks[0],id:String(i)})),800);expect(get(musicQueue).tracks).toHaveLength(300);expect(get(musicQueue).tracks[get(musicQueue).index].id).toBe('800');
    playMusic(Array.from({length:700},()=>tracks[0]));enqueueMusic(tracks[1]);expect(get(musicQueue).tracks).toHaveLength(500);clearMusic();expect(get(musicQueue).tracks).toEqual([]);
  });
});

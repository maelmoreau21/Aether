export interface LyricLine {time:number|null;text:string}
export function parseLyrics(text:string):LyricLine[]{
  const offsetMatch=/^\[offset:([+-]?\d+)\]/mi.exec(text);
  const offset=offsetMatch?Math.max(-30000,Math.min(30000,Number(offsetMatch[1])))/1000:0;
  const lines:LyricLine[]=[];
  for(const line of text.slice(0,32768).split(/\r?\n/).slice(0,2000)){
    const stamps=[...line.matchAll(/\[(\d{1,3}):([0-5]\d)(?:[.:](\d{1,3}))?\]/g)];
    if(stamps.length){const lyric=line.replace(/\[[^\]]*\]/g,'').trim();for(const stamp of stamps)lines.push({time:Math.max(0,Number(stamp[1])*60+Number(stamp[2])+Number(`0.${stamp[3]??'0'}`)-offset),text:lyric});}
    else if(line.trim()&&!/^\[[a-z]+:/i.test(line))lines.push({time:null,text:line.trim()});
  }
  return lines.some(line=>line.time!==null)?lines.filter(line=>line.time!==null).sort((a,b)=>a.time!-b.time!):lines;
}

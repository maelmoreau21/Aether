// Conservative accounting of encoded bytes submitted to MSE. Browser decoder
// surfaces and OS allocations are separate and cannot be bounded by this ledger.
export class EncodedVideoBuffer {
  private blocks: {bytes:number;end:number;pinned:boolean}[]=[];
  private retained=0;
  constructor(private readonly limit=128*1024*1024) {}
  get bytes(){return this.retained;}
  append(bytes:number,end:number) {
    if(!Number.isSafeInteger(bytes)||bytes<=0||!Number.isSafeInteger(end)||end<bytes)throw new Error('Comptabilité du tampon vidéo invalide.');
    if(bytes>this.limit-this.retained)throw new Error('Limite du tampon vidéo encodé atteinte (128 Mio) : aucun point d’accès sûr ne permet de libérer le GOP courant.');
    const last=this.blocks.at(-1);
    // Highly fragmented HTTP delivery must not create millions of tiny records.
    // Merging delays release, which deliberately overestimates retained bytes.
    if(last&&last.bytes<65536){last.bytes+=bytes;last.end=end;}
    else this.blocks.push({bytes,end,pinned:this.blocks.length===0});
    this.retained+=bytes;
  }
  evictedBefore(offset:number) {
    // Keep the initialization block counted: codec configuration can remain
    // useful after all of the first GOP's coded frames have been evicted.
    this.blocks=this.blocks.filter(block=>{if(!block.pinned&&block.end<=offset){this.retained-=block.bytes;return false;}return true;});
  }
  clear(){this.blocks=[];this.retained=0;}
}

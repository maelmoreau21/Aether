// Inspect bounded moov/moof boxes while forwarding mdat bytes without retaining
// them. Keyframe timestamps let MSE eviction stop at a known past random-access
// point instead of accidentally removing dependencies of the current frame.
export class Mp4Fragments {
  private pending = new Uint8Array();
  private remaining = 0;
  private captured?: Uint8Array;
  private cursor = 0;
  private type = '';
  private offset = 0;
  private start = 0;
  constructor(private complete: (type: string, bytes: ArrayBuffer, start:number) => void) {}
  push(input: Uint8Array) {
    let at = 0;
    while (at < input.length) {
      if (!this.remaining) {
        if(!this.pending.length)this.start=this.offset;
        const need = this.pending.length >= 8 && new DataView(this.pending.buffer).getUint32(0) === 1 ? 16 : 8;
        const take = Math.min(need - this.pending.length, input.length - at);
        const header = new Uint8Array(this.pending.length + take);header.set(this.pending);header.set(input.subarray(at, at + take), this.pending.length);this.pending = header;at += take;this.offset+=take;
        if (this.pending.length < need) continue;
        const view = new DataView(this.pending.buffer);let size = view.getUint32(0);
        if (size === 1 && need === 8) continue;
        if (size === 1) size = Number(view.getBigUint64(8));
        if (!Number.isSafeInteger(size) || size < need) throw new Error('Taille fMP4 progressive invalide.');
        this.type = String.fromCharCode(...this.pending.subarray(4, 8));
        if (this.type === 'moov' || this.type === 'moof') {
          if (size > 1024 * 1024) throw new Error('Index fMP4 supérieur à 1 Mio.');
          this.captured = new Uint8Array(size);this.captured.set(this.pending);this.cursor = need;
        }
        this.remaining = size - need;this.pending = new Uint8Array();
        if (!this.remaining) this.finish();
      } else {
        const take = Math.min(this.remaining, input.length - at);
        if (this.captured) {this.captured.set(input.subarray(at, at + take), this.cursor);this.cursor += take;}
        at += take;this.offset+=take;this.remaining -= take;if (!this.remaining) this.finish();
      }
    }
  }
  private finish() {if (this.captured) this.complete(this.type, this.captured.buffer as ArrayBuffer,this.start);this.captured = undefined;}
  finishStream() {if (this.remaining || this.pending.length) throw new Error('Flux fMP4 tronqué.');}
}
type Box = {start: number; end: number; type: string};
function children(data: ArrayBuffer, start: number, end: number): Box[] {
  const result: Box[] = [],view = new DataView(data);
  while (start < end) {
    if (end - start < 8 || result.length >= 16384) throw new Error('Index fMP4 invalide.');
    const size = view.getUint32(start);
    if (size < 8 || size > end - start) throw new Error('Boîte fMP4 tronquée.');
    result.push({start: start + 8, end: start + size, type: String.fromCharCode(...new Uint8Array(data, start + 4, 4))});start += size;
  }return result;
}
function child(data: ArrayBuffer, parent: Box, type: string): Box {
  const found = children(data, parent.start, parent.end).find(box => box.type === type);
  if (!found) throw new Error(`Index fMP4 sans ${type}.`);return found;
}
export function mp4Timescale(data: ArrayBuffer): number {
  const moov = children(data, 0, data.byteLength).find(box => box.type === 'moov');
  if (!moov) throw new Error('Initialisation fMP4 absente.');
  const mdhd = child(data, child(data, child(data, moov, 'trak'), 'mdia'), 'mdhd'),view = new DataView(data);
  const version = view.getUint8(mdhd.start),at = mdhd.start + (version === 1 ? 20 : 12);
  if (version > 1 || at + 4 > mdhd.end) throw new Error('Horloge fMP4 incompatible.');
  const scale = view.getUint32(at);if (!scale) throw new Error('Horloge fMP4 nulle.');return scale;
}
export function mp4RandomAccess(data: ArrayBuffer, scale: number): number | undefined {
  const moof = children(data, 0, data.byteLength).find(box => box.type === 'moof');if (!moof) return;
  const traf = child(data, moof, 'traf'),tfhd = child(data, traf, 'tfhd'),tfdt = child(data, traf, 'tfdt'),trun = child(data, traf, 'trun'),view = new DataView(data);
  const flags = (at: number) => view.getUint32(at) & 0xffffff;
  const headerFlags = flags(tfhd.start);let at = tfhd.start + 8;
  if (headerFlags & 1) at += 8;if (headerFlags & 2) at += 4;if (headerFlags & 8) at += 4;if (headerFlags & 16) at += 4;
  let sampleFlags = headerFlags & 32 ? view.getUint32(at) : undefined;
  const runFlags = flags(trun.start);at = trun.start + 8;
  if (runFlags & 1) at += 4;if (runFlags & 4) {sampleFlags = view.getUint32(at);at += 4;}
  if (runFlags & 256) at += 4;if (runFlags & 512) at += 4;
  if (runFlags & 1024) {sampleFlags = view.getUint32(at);at += 4;}
  if (sampleFlags === undefined || (sampleFlags & 0x10000) || ((sampleFlags >>> 24) & 3) !== 2) return;
  const decode = view.getUint8(tfdt.start) === 1 ? Number(view.getBigUint64(tfdt.start + 4)) : view.getUint32(tfdt.start + 4);
  const composition = runFlags & 2048 ? (view.getUint8(trun.start) === 1 ? view.getInt32(at) : view.getUint32(at)) : 0;
  const time = (decode + composition) / scale;
  if (!Number.isFinite(time) || time < 0) throw new Error('Horodatage fMP4 invalide.');return time;
}

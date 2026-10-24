// FFmpeg delay_moov can represent a source start with an empty edit while tfdt
// starts at zero. MSE does not apply that movie edit to SourceBuffer timestamps.
// Accept only the generated video-only subset; never guess an unsupported edit.
export function mp4TimelineOffset(data: ArrayBuffer): number {
  const view = new DataView(data);
  let inspected = 0;
  type Box = { start: number; end: number; type: string };
  const boxes = (start: number, end: number): Box[] => {
    const result: Box[] = [];
    while (start < end) {
      if (++inspected > 100000 || end - start < 8) throw new Error('Index MP4 fragmenté invalide.');
      let size = view.getUint32(start), header = 8;
      const type = String.fromCharCode(...new Uint8Array(data, start + 4, 4));
      if (size === 1) { if (end - start < 16) throw new Error('Boîte MP4 tronquée.'); size = Number(view.getBigUint64(start + 8)); header = 16; }
      if (size === 0) size = end - start;
      if (!Number.isSafeInteger(size) || size < header || size > end - start) throw new Error('Taille de boîte MP4 invalide.');
      result.push({ start: start + header, end: start + size, type });start += size;
    }
    return result;
  };
  const requireBox = (parent: Box, type: string) => {
    const box = boxes(parent.start, parent.end).find(box => box.type === type);
    if (!box) throw new Error(`Index MP4 sans ${type}.`);
    return box;
  };
  const root = {start: 0, end: data.byteLength, type: ''};
  const moov = requireBox(root, 'moov');
  const mvhd = requireBox(moov, 'mvhd');
  if (mvhd.end - mvhd.start < 20) throw new Error('Horloge MP4 tronquée.');
  const version = view.getUint8(mvhd.start);
  if (version > 1 || mvhd.end - mvhd.start < (version ? 24 : 16)) throw new Error('Horloge MP4 incompatible.');
  const timescale = view.getUint32(mvhd.start + (version ? 20 : 12));
  if (!timescale) throw new Error('Horloge MP4 nulle.');
  const tracks = boxes(moov.start, moov.end).filter(box => box.type === 'trak');
  if (tracks.length !== 1) throw new Error('Fenêtre vidéo MP4 avec plusieurs pistes.');
  const edts = boxes(tracks[0].start, tracks[0].end).find(box => box.type === 'edts');
  if (!edts) return 0;
  const elst = requireBox(edts, 'elst');
  if (elst.end - elst.start < 8) throw new Error('Édition MP4 tronquée.');
  const editVersion = view.getUint8(elst.start), count = view.getUint32(elst.start + 4);
  if (editVersion > 1 || count < 1 || count > 2) throw new Error('Édition MP4 incompatible.');
  const stride = editVersion ? 20 : 12;
  if (elst.end - elst.start < 8 + count * stride) throw new Error('Édition MP4 tronquée.');
  let emptyDuration = 0, mediaSeen = false;
  for (let i = 0; i < count; i++) {
    const entry = elst.start + 8 + i * stride;
    const duration = editVersion ? Number(view.getBigUint64(entry)) : view.getUint32(entry);
    const mediaTime = editVersion ? Number(view.getBigInt64(entry + 8)) : view.getInt32(entry + 4);
    const rate = view.getInt32(entry + (editVersion ? 16 : 8));
    if (rate !== 65536 || !Number.isSafeInteger(duration)) throw new Error('Vitesse d’édition MP4 incompatible.');
    if (mediaTime === -1 && !mediaSeen) emptyDuration += duration;
    else if (mediaTime === 0 && !mediaSeen) mediaSeen = true;
    else throw new Error('Origine d’édition MP4 incompatible.');
  }
  const offset = emptyDuration / timescale;
  if (!mediaSeen || !Number.isFinite(offset) || offset < 0 || offset > 86400) throw new Error('Origine MP4 invalide.');
  return offset;
}

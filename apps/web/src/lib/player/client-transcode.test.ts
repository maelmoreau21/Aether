import { describe, expect, it } from 'vitest';
import {
  buildFmp4InitSegment,
  buildFmp4MediaSegment,
  canClientTranscode
} from './client-transcode';

function readBoxTypes(bytes: Uint8Array): string[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const types: string[] = [];
  let offset = 0;
  while (offset + 8 <= bytes.byteLength) {
    const size = view.getUint32(offset);
    if (size < 8 || offset + size > bytes.byteLength) break;
    types.push(String.fromCharCode(...bytes.subarray(offset + 4, offset + 8)));
    offset += size;
  }
  return types;
}

describe('client-side transcoding helpers', () => {
  it('checks required WebCodecs capabilities per stream kind', () => {
    expect(canClientTranscode(undefined, true, false)).toBe(false);
    expect(canClientTranscode({ video: false, audio: true }, true, false)).toBe(false);
    expect(canClientTranscode({ video: false, audio: true }, false, true)).toBe(true);
    expect(canClientTranscode({ video: true, audio: false }, true, false)).toBe(true);
    expect(canClientTranscode({ video: true, audio: true }, true, true)).toBe(true);
    expect(canClientTranscode({ video: true, audio: true }, false, false)).toBe(false);
  });

  it('builds valid ftyp+moov init segments and moof+mdat media segments', () => {
    const init = buildFmp4InitSegment(
      { trackId: 1, timescale: 90000, width: 1280, height: 720, avcC: new Uint8Array([1, 100, 0, 41]) },
      { trackId: 2, timescale: 48000, channels: 2, sampleRate: 48000, asc: new Uint8Array([0x11, 0x90]) }
    );
    expect(readBoxTypes(init)).toEqual(['ftyp', 'moov']);

    const media = buildFmp4MediaSegment(1, 1, 0, [
      { data: new Uint8Array([0, 0, 0, 2, 0x65, 0x88]), duration: 3000, isKey: true }
    ]);
    expect(readBoxTypes(media)).toEqual(['moof', 'mdat']);
  });
});

import { describe, expect, it } from 'vitest';
import { decodeObject, displaySetAt, paletteColor, parsePgs } from './pgs';

function segment(type: number, pts: number, payload: number[]): number[] {
  return [0x50, 0x47, (pts >>> 24) & 255, (pts >>> 16) & 255, (pts >>> 8) & 255, pts & 255, 0, 0, 0, 0, type, payload.length >> 8, payload.length & 255, ...payload];
}
const u16 = (value: number) => [value >> 8, value & 255];

/** One 4×2 object at (100, 900): a red run and a transparent run on each line. */
function displaySet(pts: number, objects: number): number[] {
  const pcs = [...u16(1920), ...u16(1080), 0x10, ...u16(1), 0x80, 0, 0, objects,
    ...(objects ? [...u16(7), 0, 0, ...u16(100), ...u16(900)] : [])];
  if (!objects) return [...segment(0x16, pts, pcs), ...segment(0x80, pts, [])];
  const pds = [0, 0, 1, 81, 240, 90, 255];
  // Line: colour 1 ×2 (0x00 0x82 0x01), colour 0 ×2 (0x00 0x02), end of line (0x00 0x00).
  const line = [0x00, 0x82, 0x01, 0x00, 0x02, 0x00, 0x00];
  const rle = [...line, ...line];
  const ods = [...u16(7), 0, 0xc0, 0, (rle.length + 4) >> 8, (rle.length + 4) & 255, ...u16(4), ...u16(2), ...rle];
  return [...segment(0x16, pts, pcs), ...segment(0x14, pts, pds), ...segment(0x15, pts, ods), ...segment(0x80, pts, [])];
}

describe('PGS subtitles', () => {
  const bytes = new Uint8Array([...displaySet(90000, 1), ...displaySet(270000, 0)]);
  const sets = parsePgs(bytes);

  it('parses display sets with their palette, objects and positions', () => {
    expect(sets.map(set => set.time)).toEqual([1, 3]);
    expect(sets[0]).toMatchObject({ width: 1920, height: 1080, compositions: [{ object: 7, x: 100, y: 900 }] });
    expect(sets[0].objects.get(7)).toMatchObject({ width: 4, height: 2 });
    expect(sets[1].compositions).toEqual([]);
  });

  it('decodes run-length lines with the palette', () => {
    const set = sets[0];
    const pixels = decodeObject(set.objects.get(7)!, set.palette);
    const red = set.palette[1];
    expect(Array.from(pixels)).toEqual([red, red, 0, 0, red, red, 0, 0]);
    // Opaque and red-dominant in BT.709 limited range.
    expect(red >>> 24).toBe(255);
    expect(red & 255).toBeGreaterThan(200);
    expect((red >>> 8) & 255).toBeLessThan(60);
  });

  it('finds the set shown at a time, clearing at an empty composition', () => {
    expect(displaySetAt(sets, 0.5)).toBeUndefined();
    expect(displaySetAt(sets, 2)).toBe(sets[0]);
    expect(displaySetAt(sets, 3)?.compositions).toEqual([]);
  });

  it('stops at malformed or truncated data instead of throwing', () => {
    expect(parsePgs(new Uint8Array([1, 2, 3]))).toEqual([]);
    expect(parsePgs(bytes.subarray(0, bytes.length - 5)).length).toBe(1);
  });

  it('converts white and black limited-range entries', () => {
    expect(paletteColor(235, 128, 128, 255, true)).toBe(0xffffffff);
    expect(paletteColor(16, 128, 128, 128, true)).toBe(0x80000000);
  });
});

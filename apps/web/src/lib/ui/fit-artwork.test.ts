import { describe, expect, it } from 'vitest';
import { artworkRatio, artworkShape, cropFraction } from './fit-artwork';

describe('fit-artwork', () => {
  it('classe les ratios natifs courants', () => {
    expect(artworkShape(2 / 3)).toBe('portrait');
    expect(artworkShape(1)).toBe('square');
    expect(artworkShape(16 / 9)).toBe('landscape');
    expect(artworkShape(4 / 3)).toBe('landscape');
    expect(artworkShape(2.39)).toBe('wide');
    expect(artworkShape(0)).toBeNull();
    expect(artworkShape(Number.NaN)).toBeNull();
  });

  it('publie un ratio CSS borné', () => {
    expect(artworkRatio(1920, 1080)).toBe('1.7778');
    expect(artworkRatio(1000, 1500)).toBe('0.6667');
    expect(artworkRatio(10000, 100)).toBe('2.4');
    expect(artworkRatio(0, 100)).toBeNull();
  });

  it('mesure le rognage d\'un cadre incompatible', () => {
    expect(cropFraction(16 / 9, 16 / 9)).toBe(0);
    expect(cropFraction(2 / 3, 16 / 9)).toBeCloseTo(0.625, 3);
  });
});

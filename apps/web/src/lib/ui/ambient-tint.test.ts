import { describe, expect, it } from 'vitest';
import { averageTint } from './ambient-tint';

describe('teinte d’ambiance', () => {
  it('keeps the hue of a dark image and lifts it to a visible level', () => {
    const pixels = new Uint8ClampedArray([60, 20, 10, 255, 60, 20, 10, 255]);
    expect(averageTint(pixels)).toBe('rgb(150 50 25)');
  });

  it('ignores transparent pixels and returns nothing for an empty or black image', () => {
    expect(averageTint(new Uint8ClampedArray([255, 0, 0, 0]))).toBeNull();
    expect(averageTint(new Uint8ClampedArray([0, 0, 0, 255]))).toBeNull();
    expect(averageTint([])).toBeNull();
  });

  it('weights saturated pixels above grey ones', () => {
    const tint = averageTint(new Uint8ClampedArray([200, 200, 200, 255, 220, 40, 40, 255]));
    const [red, green] = tint!.match(/\d+/g)!.map(Number);
    expect(red).toBeGreaterThan(green + 40);
  });
});

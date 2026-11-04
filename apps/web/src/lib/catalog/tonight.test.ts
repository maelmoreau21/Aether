import { describe, expect, it } from 'vitest';
import type { Work } from './api';
import { mosaicLayout, pickTonight } from './tonight';

const work = (id: string, kind: 'film' | 'series', addedAt?: number): Work => ({ id, kind, title: id, year: null, description: '', libraryId: 'lib', synthetic: false, addedAt });
const withArt = () => true;

describe('pour ce soir', () => {
  it('mixes recent films and series and skips the works already on the hero', () => {
    const films = [work('f1', 'film', 9), work('f2', 'film', 7), work('f3', 'film', 5), work('f4', 'film', 3)];
    const series = [work('s1', 'series', 8), work('s2', 'series', 6), work('s3', 'series', 4), work('s4', 'series', 2)];
    expect(pickTonight(films, series, ['f1'], withArt).map(item => item.id)).toEqual(['s1', 'f2', 's2', 'f3', 's3', 'f4', 's4']);
  });

  it('prefers works with artwork and completes with hero works when needed', () => {
    const films = [work('f1', 'film'), work('f2', 'film'), work('f3', 'film')];
    const series = [work('s1', 'series')];
    const picked = pickTonight(films, series, ['f1'], item => item.id !== 'f2');
    expect(picked.map(item => item.id)).toEqual(['s1', 'f3', 'f2', 'f1']);
  });

  it('hides the section below four works', () => {
    expect(pickTonight([work('f1', 'film'), work('f2', 'film')], [work('s1', 'series')], [], withArt)).toEqual([]);
  });

  it('fills every band without gaps on each screen size', () => {
    const columns = { desktop: 6, tablet: 4, phone: 2 } as const;
    for (let count = 4; count <= 9; count++) {
      const tiles = mosaicLayout(count);
      expect(tiles.length).toBe(Math.min(7, count));
      expect(tiles[0].shape).toBe('wide');
      for (const breakpoint of ['desktop', 'tablet', 'phone'] as const) {
        const visible = tiles.filter(tile => !tile.hidden[breakpoint]);
        expect(visible.length).toBeGreaterThanOrEqual(4);
        let used = 0;
        for (const tile of visible) {
          const span = tile.span[breakpoint];
          expect(span).toBeLessThanOrEqual(columns[breakpoint]);
          used = used + span > columns[breakpoint] ? span : used + span;
          expect(used).toBeLessThanOrEqual(columns[breakpoint]);
        }
        expect(used).toBe(columns[breakpoint]);
        expect(visible.reduce((total, tile) => total + tile.span[breakpoint], 0) % columns[breakpoint]).toBe(0);
      }
    }
    expect(mosaicLayout(3)).toEqual([]);
  });
});

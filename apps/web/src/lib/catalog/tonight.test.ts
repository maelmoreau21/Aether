import { describe, expect, it } from 'vitest';
import type { Work } from './api';
import { arrangeTonight, mosaicLayout, pickTonight, pickTonightFrom } from './tonight';

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

  it('uses one 16:9 feature then only 2:3 posters, filling each grid exactly', () => {
    for (let count = 4; count <= 9; count++) {
      const layout = mosaicLayout(count);
      if (!layout) throw new Error(`no layout for ${count}`);
      expect(layout.tiles.length).toBe(Math.min(7, count));
      expect(layout.tiles.map(tile => tile.shape)).toEqual(['feature', ...Array(layout.posters).fill('poster')]);
      expect(layout.tiles[0].hidden).toEqual({ desktop: false, tablet: false, phone: false });
      for (const breakpoint of ['desktop', 'tablet', 'phone'] as const) {
        const { columns, featureRows } = layout.grids[breakpoint];
        const visible = layout.tiles.filter(tile => tile.shape === 'poster' && !tile.hidden[breakpoint]).length;
        expect(visible).toBeGreaterThanOrEqual(3);
        expect(visible % columns).toBe(0);
        // Ordinateur : la vedette couvre exactement les rangées d'affiches à côté d'elle.
        if (featureRows) expect(visible).toBe(columns * featureRows);
      }
      // Ordinateur et tablette : aucune œuvre masquée.
      expect(layout.tiles.every(tile => !tile.hidden.desktop && !tile.hidden.tablet)).toBe(true);
    }
    expect(mosaicLayout(3)).toBeNull();
  });

  it('keeps all seven works visible on every screen size', () => {
    const layout = mosaicLayout(7);
    expect(layout?.tiles.filter(tile => tile.hidden.desktop || tile.hidden.tablet || tile.hidden.phone)).toEqual([]);
    expect(layout?.grids).toEqual({ desktop: { columns: 3, featureRows: 2 }, tablet: { columns: 6, featureRows: 0 }, phone: { columns: 3, featureRows: 0 } });
  });

  it('features the first work that has landscape artwork', () => {
    const works = [work('a', 'film'), work('b', 'series'), work('c', 'film')];
    expect(arrangeTonight(works, item => item.id === 'b').map(item => item.id)).toEqual(['b', 'a', 'c']);
    expect(arrangeTonight(works, () => false)).toBe(works);
    expect(arrangeTonight(works, () => true)).toBe(works);
  });

  it('keeps the order of a random sample, without the hero works or duplicates', () => {
    const pool = ['x', 'h', 'b', 'x', 'c', 'd', 'e', 'f', 'g', 'i'].map(id => work(id, 'film'));
    expect(pickTonightFrom(pool, ['h'], item => item.id !== 'b').map(item => item.id)).toEqual(['x', 'c', 'd', 'e', 'f', 'g', 'i']);
    expect(pickTonightFrom(pool.slice(0, 5), ['h'], withArt).map(item => item.id)).toEqual(['x', 'b', 'c', 'h']);
    expect(pickTonightFrom(pool.slice(0, 4), ['h'], withArt)).toEqual([]);
  });
});

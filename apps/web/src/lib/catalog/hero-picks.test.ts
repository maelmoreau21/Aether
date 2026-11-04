import { describe, expect, it } from 'vitest';
import type { Work } from './api';
import { HERO_COUNT, pickHero, shuffleWithSeed } from './hero-picks';

const work = (id: string, kind: 'film' | 'series', wide = true) => ({ id, kind, wide } as unknown as Work & { wide: boolean });
const films = Array.from({ length: 10 }, (_, index) => work(`f${index}`, 'film'));
const series = Array.from({ length: 10 }, (_, index) => work(`s${index}`, 'series'));
const wide = (item: Work) => (item as Work & { wide: boolean }).wide;

describe('pickHero', () => {
  it('picks four distinct works, alternating films and series', () => {
    const picked = pickHero(films, series, wide, 7);
    expect(picked).toHaveLength(HERO_COUNT);
    expect(new Set(picked.map(item => item.id)).size).toBe(HERO_COUNT);
    expect(picked.filter(item => item.kind === 'film')).toHaveLength(2);
  });

  it('is stable for a seed and varies across seeds', () => {
    expect(pickHero(films, series, wide, 42).map(item => item.id)).toEqual(pickHero(films, series, wide, 42).map(item => item.id));
    const draws = new Set(Array.from({ length: 20 }, (_, seed) => pickHero(films, series, wide, seed).map(item => item.id).join()));
    expect(draws.size).toBeGreaterThan(10);
  });

  it('skips works without wide artwork and fills from the other kind', () => {
    const fewSeries = [work('s0', 'series'), work('s1', 'series', false)];
    const picked = pickHero(films, fewSeries, wide, 3);
    expect(picked).toHaveLength(HERO_COUNT);
    expect(picked.map(item => item.id)).not.toContain('s1');
    expect(picked.map(item => item.id)).toContain('s0');
  });

  it('returns fewer works when the catalogue is small', () => {
    expect(pickHero([films[0]], [], wide, 1)).toHaveLength(1);
    expect(pickHero([], [], wide, 1)).toEqual([]);
  });
});

describe('shuffleWithSeed', () => {
  it('keeps every item', () => {
    expect(shuffleWithSeed(films, 9).map(item => item.id).sort()).toEqual(films.map(item => item.id).sort());
  });
});

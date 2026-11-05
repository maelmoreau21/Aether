import { describe, expect, it } from 'vitest';
import { buildDiscoverPlan, shuffleWithSeed } from './discover';
import type { Work } from './api';

const work = (id: string, kind: 'film' | 'series', genres: string[]): Work => ({ id, kind, title: id, genres } as Work);

describe('discover feed plan', () => {
  it('shuffles deterministically without losing items', () => {
    const items = Array.from({ length: 20 }, (_, index) => index);
    const first = shuffleWithSeed(items, 'seed');
    expect(shuffleWithSeed(items, 'seed')).toEqual(first);
    expect([...first].sort((a, b) => a - b)).toEqual(items);
    expect(shuffleWithSeed(items, 'other')).not.toEqual(first);
  });

  it('alternates film and series genres and inserts special shelves', () => {
    const works = [
      work('a', 'film', ['Action', 'Drame']), work('b', 'film', ['Action']), work('c', 'film', ['Comédie']),
      work('d', 'series', ['Drame']), work('e', 'series', ['Animation'])
    ];
    const plan = buildDiscoverPlan(works, 'visit');
    const keys = plan.map(spec => spec.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(plan.filter(spec => spec.genre && spec.kind === 'film').map(spec => spec.genre).sort()).toEqual(['Action', 'Comédie', 'Drame']);
    expect(plan.filter(spec => spec.genre && spec.kind === 'series')).toHaveLength(2);
    expect(plan[0].kind).toBe('film');
    expect(plan[1].kind).toBe('series');
    expect(keys).toContain('film:rediscover');
    expect(keys).toContain('series:rediscover');
  });

  it('omits a kind absent from the library', () => {
    const plan = buildDiscoverPlan([work('a', 'film', ['Action'])], 'visit');
    expect(plan.every(spec => spec.kind === 'film')).toBe(true);
    expect(plan.length).toBeGreaterThan(1);
  });
});

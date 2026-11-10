import { describe, expect, it } from 'vitest';
import type { Work } from './api';
import { pickSurprise, surpriseCandidates, surpriseGenres } from './surprise';

const work = (id: string, kind: Work['kind'], genres: string[] = []) => ({ id, kind, title: id, year: null, genres }) as unknown as Work;
const works = [work('a', 'film', ['Drame']), work('b', 'series', ['Comédie']), work('c', 'film', ['Comédie', 'Drame'])];

describe('surprise', () => {
  it('lists unique genres in order', () => expect(surpriseGenres(works)).toEqual(['Comédie', 'Drame']));
  it('filters by kind, genre and seen history', () => {
    expect(surpriseCandidates(works, { kind: 'film', genre: 'Comédie', unseenOnly: false }).map(w => w.id)).toEqual(['c']);
    const seen = [{ workId: 'a' }, { workId: 'ep1', seriesId: 'b' }];
    expect(surpriseCandidates(works, { kind: 'all', genre: '', unseenOnly: true }, seen).map(w => w.id)).toEqual(['c']);
  });
  it('avoids the previous pick when possible', () => {
    expect(pickSurprise(works.slice(0, 2), 'a', () => 0)?.id).toBe('b');
    expect(pickSurprise([works[0]], 'a', () => 0.99)?.id).toBe('a');
    expect(pickSurprise([], undefined)).toBeNull();
  });
});

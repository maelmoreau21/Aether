import { describe, expect, it } from 'vitest';
import { metadataFormFromWork, metadataPayloadFromForm, splitCsv } from './work-metadata';
import type { Work } from './api';

describe('work metadata form', () => {
  it('splits comma separated lists', () => {
    expect(splitCsv(' a, b ,, c ')).toEqual(['a', 'b', 'c']);
    expect(splitCsv('')).toEqual([]);
  });

  it('fills the form from a work and hides the placeholder synopsis', () => {
    const work = { title: 'T', description: 'Média local indexé', genres: ['A', 'B'], tags: [], externalIds: { tmdb: '1' }, year: 1999, minimumAge: 12 } as unknown as Work;
    const form = metadataFormFromWork(work);
    expect(form.plot).toBe('');
    expect(form.genres).toBe('A, B');
    expect(form.year).toBe(1999);
    expect(form.age).toBe(12);
    expect(JSON.parse(form.ids)).toEqual({ tmdb: '1' });
    expect(metadataFormFromWork({ title: 'X' } as unknown as Work).ids).toBe('{}');
  });

  it('builds the payload or rejects invalid external ids', () => {
    const form = { title: 'T', plot: 'p', year: undefined, genres: 'a, b', tags: '', ids: '{"x":1}', age: undefined };
    expect(metadataPayloadFromForm(form)).toEqual({
      title: 'T', plot: 'p', year: null, genres: ['a', 'b'], tags: [], externalIds: { x: 1 }, minimumAge: null
    });
    expect(metadataPayloadFromForm({ ...form, ids: '{oops' })).toBeNull();
  });
});

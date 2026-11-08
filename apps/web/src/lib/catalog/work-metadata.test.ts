import { describe, expect, it } from 'vitest';
import {
  buildMetadataPutPayload,
  createMetadataDraft,
  metadataFormFromWork,
  metadataPayloadFromForm,
  splitCsv,
  switchMetadataDraftLocale
} from './work-metadata';
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

  it('supports multi-language metadata draft switching and full Jellyfin-style payload generation', () => {
    const work: Work = {
      id: 'work-1',
      kind: 'film',
      title: 'La Bataille de Gaulle',
      description: 'Juin 1940.',
      libraryId: 'lib-1',
      synthetic: false,
      year: 2026,
      genres: ['Drama', 'History', 'War'],
      externalIds: { imdb: 'tt35856133', tmdb: '1165369' }
    };

    const draft = createMetadataDraft(
      work,
      {
        workId: 'work-1',
        libraryId: 'lib-1',
        kind: 'film',
        preferredMetadataLanguage: 'fr',
        title: 'La Bataille de Gaulle',
        sortTitle: 'la bataille de gaulle',
        originalLanguage: 'fr',
        premiered: '2026-06-26',
        rating: 7.842,
        mpaa: 'TP',
        collection: { id: '1165374', name: 'De Gaulle' },
        actors: [{ name: 'Simon Abkarian', role: 'Charles de Gaulle' }],
        directors: ['Antonin Baudry'],
        locked: false,
        lockedFields: ['mpaa'],
        translations: {
          fr: { title: 'La Bataille de Gaulle', plot: 'Juin 1940.' },
          en: { title: 'The Battle of de Gaulle', plot: 'June 1940.' }
        }
      },
      'fr'
    );

    expect(draft.activeLocale).toBe('fr');
    expect(draft.title).toBe('La Bataille de Gaulle');
    expect(draft.imdbId).toBe('tt35856133');
    expect(draft.tmdbId).toBe('1165369');
    expect(draft.collectionId).toBe('1165374');

    switchMetadataDraftLocale(draft, 'en');
    expect(draft.activeLocale).toBe('en');
    expect(draft.title).toBe('The Battle of de Gaulle');
    draft.tagline = 'Free France';

    const payload = buildMetadataPutPayload(draft);
    expect(payload.title).toBe('La Bataille de Gaulle');
    expect(payload.mpaa).toBe('TP');
    expect(payload.lockedFields).toEqual(['mpaa']);
    expect(payload.collection).toEqual({ id: '1165374', name: 'De Gaulle' });
    expect((payload.translations as Record<string, { tagline?: string }>).en.tagline).toBe('Free France');
  });
});

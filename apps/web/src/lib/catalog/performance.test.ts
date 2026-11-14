import { describe, expect, it } from 'vitest';
import type { Work } from './api';
import { selectTopRatedSample } from './CatalogHome.svelte';
import { resolveShelfBatch } from './DiscoverFeed.svelte';
import { resolvePosterPresentation } from './Poster.svelte';
import { mosaicTileCandidates } from './TonightMosaic.svelte';

const work = (id: string, rating?: number): Work => ({
  id,
  kind: 'film',
  title: id,
  year: 2024,
  description: '',
  libraryId: 'lib',
  synthetic: false,
  rating
});

describe('CatalogHome single-request top rated prioritization', () => {
  it('keeps only >= 7 items when at least 8 meet the high rating threshold', () => {
    const input = [
      work('low-1', 6.4),
      ...Array.from({ length: 8 }, (_, index) => work(`top-${index}`, 7 + index * 0.1)),
      work('low-2', 6.8)
    ];

    const selected = selectTopRatedSample(input);
    expect(selected.map(item => item.id)).toEqual(
      Array.from({ length: 8 }, (_, index) => `top-${index}`)
    );
  });

  it('prioritizes >= 7 items and backfills with >= 6 items when fewer than 8 reach 7', () => {
    const input = [
      work('six-a', 6.9),
      work('seven-a', 8.2),
      work('below-six', 5.5),
      work('six-b', 6.1),
      work('seven-b', 7.0)
    ];

    const selected = selectTopRatedSample(input);
    expect(selected.map(item => item.id)).toEqual(['seven-a', 'seven-b', 'six-a', 'six-b']);
  });
});

describe('Poster zero-ms cached presentation', () => {
  it('shows cached images synchronously without transition even before onload fires', () => {
    expect(resolvePosterPresentation('/img?width=320', '', '', true)).toEqual({
      loaded: true,
      instant: true
    });
    expect(resolvePosterPresentation('/img?width=320', '', '/img?width=320', false)).toEqual({
      loaded: true,
      instant: true
    });
  });

  it('keeps smooth fade-in transition for uncached network images', () => {
    expect(resolvePosterPresentation('/img?width=320', '', '', false)).toEqual({
      loaded: false,
      instant: false
    });
    expect(resolvePosterPresentation('/img?width=320', '/img?width=320', '', false)).toEqual({
      loaded: true,
      instant: false
    });
  });
});

describe('TonightMosaic feature preview priority', () => {
  it('prioritizes 1280px preview for the feature tile before raw landscape', () => {
    expect(
      mosaicTileCandidates(
        {
          landscape: '/api/v1/assets/wide/image',
          backdrop: '/api/v1/assets/bg/image',
          poster: '/api/v1/assets/post/image'
        },
        true
      )
    ).toEqual([
      '/api/v1/assets/wide/image?width=1280',
      '/api/v1/assets/wide/image',
      '/api/v1/assets/post/image?width=1280',
      '/api/v1/assets/post/image'
    ]);
  });
});

describe('DiscoverFeed parallel batch resolution', () => {
  it('stops after consuming the first valid shelf in a parallel batch', () => {
    const seen = new Set<string>(['already-shown']);
    const itemsA = [work('a1'), work('a2'), work('a3'), work('a4')];
    const itemsB = [work('b1'), work('b2'), work('b3'), work('b4')];
    const outcome = resolveShelfBatch(
      [
        { spec: { key: 's1', title: 'S1', kind: 'film', sort: 'added-desc' }, result: { status: 'fulfilled', value: { items: itemsA } } },
        { spec: { key: 's2', title: 'S2', kind: 'film', sort: 'added-desc' }, result: { status: 'fulfilled', value: { items: itemsB } } }
      ],
      seen,
      'seed-1',
      4
    );

    expect(outcome.kind).toBe('shelf');
    if (outcome.kind === 'shelf') {
      expect(outcome.consumed).toBe(1);
      expect(outcome.shelf.key).toBe('s1');
      expect(outcome.shelf.items).toHaveLength(4);
    }
  });

  it('skips an undersized first shelf and promotes the parallel second candidate in one round', () => {
    const seen = new Set<string>(['dup']);
    const shortList = [work('dup'), work('only-1')];
    const validList = [work('b1'), work('b2'), work('b3'), work('b4')];
    const outcome = resolveShelfBatch(
      [
        { spec: { key: 's1', title: 'S1', kind: 'film', sort: 'random' }, result: { status: 'fulfilled', value: { items: shortList } } },
        { spec: { key: 's2', title: 'S2', kind: 'series', sort: 'added-desc' }, result: { status: 'fulfilled', value: { items: validList } } }
      ],
      seen,
      'seed-2',
      4
    );

    expect(outcome.kind).toBe('shelf');
    if (outcome.kind === 'shelf') {
      expect(outcome.consumed).toBe(2);
      expect(outcome.shelf.key).toBe('s2');
    }
  });
});

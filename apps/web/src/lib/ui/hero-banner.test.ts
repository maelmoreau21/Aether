import { describe, expect, it } from 'vitest';
import { heroArtworkCandidates } from './HeroBanner.svelte';

describe('HeroBanner optimized preview selection', () => {
  it('prefers 1280px WebP preview URLs before raw original artwork', () => {
    const candidates = heroArtworkCandidates({
      backdrop: '/api/v1/assets/bg-1/image?v=1',
      landscape: '/api/v1/assets/land-1/image?v=2',
      poster: '/api/v1/assets/post-1/image?v=3'
    });

    expect(candidates).toEqual([
      '/api/v1/assets/bg-1/image?v=1&width=1280',
      '/api/v1/assets/land-1/image?v=2&width=1280',
      '/api/v1/assets/bg-1/image?v=1',
      '/api/v1/assets/land-1/image?v=2',
      '/api/v1/assets/post-1/image?v=3&width=1280'
    ]);
  });

  it('uses poster preview before raw poster when only poster artwork exists', () => {
    const candidates = heroArtworkCandidates({
      backdrop: null,
      landscape: null,
      poster: '/api/v1/assets/post-2/image'
    });

    expect(candidates).toEqual([
      '/api/v1/assets/post-2/image?width=1280',
      '/api/v1/assets/post-2/image'
    ]);
  });
});

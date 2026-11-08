import { describe, expect, it } from 'vitest';
import { meaningfulText, resumeTarget } from './detail-facts';
import type { Work } from './api';
import type { Progress } from '../player/progress';

const episode = (id: string, number: number, seasonNumber?: number): Work =>
  ({ id, kind: 'episode', title: `Episode ${number}`, year: null, description: '', libraryId: 'l', synthetic: false, number, seasonNumber }) as Work;

const series = {
  id: 'show', kind: 'series', title: 'Show', year: 2021, description: '', libraryId: 'l', synthetic: false,
  seasons: [
    { id: 's1', number: 1, title: '', episodes: [episode('e1', 1), episode('e2', 2)] },
    { id: 's2', number: 2, title: '', episodes: [episode('e3', 1)] }
  ]
} as Work;

const progress = (workId: string, extra: Partial<Progress> = {}): Progress =>
  ({ workId, position: 600, duration: 2400, watched: false, revision: 1, seriesId: 'show', ...extra });

const resumable = (item: Progress) => !item.watched && item.position > 0;

describe('meaningfulText', () => {
  it('rejects blank text and generic placeholders', () => {
    expect(meaningfulText('  ', [])).toBe(false);
    expect(meaningfulText(null)).toBe(false);
    expect(meaningfulText('local', ['local'])).toBe(false);
    expect(meaningfulText('plot', ['local'])).toBe(true);
  });
});

describe('resumeTarget', () => {
  it('returns the most recent in-progress episode of this series with its season', () => {
    const target = resumeTarget(series, [progress('other', { seriesId: 'elsewhere' }), progress('e3'), progress('e1')], resumable);
    expect(target?.episode.id).toBe('e3');
    expect(target?.season).toBe(2);
  });

  it('skips finished episodes', () => {
    expect(resumeTarget(series, [progress('e3', { watched: true }), progress('e2')], resumable)?.episode.id).toBe('e2');
  });

  it('finds an episode only present in the loaded page', () => {
    const paged = { ...series, seasons: [{ id: 's3', number: 3, title: '', episodes: [] }] } as Work;
    const target = resumeTarget(paged, [progress('e9')], resumable, [episode('e9', 4)], 3);
    expect(target).toMatchObject({ season: 3 });
    expect(target?.episode.number).toBe(4);
  });

  it('returns null when the episode is not loaded or nothing is in progress', () => {
    expect(resumeTarget(series, [progress('unknown')], resumable)).toBeNull();
    expect(resumeTarget(series, [], resumable)).toBeNull();
  });
});

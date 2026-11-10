import type { Work } from './api';

export type SurpriseKind = 'all' | 'film' | 'series';
export interface SurpriseFilters { kind: SurpriseKind; genre: string; unseenOnly: boolean }
type Seen = { workId: string; seriesId?: string };

/** Genres present in the catalogue, sorted for a stable picker. */
export function surpriseGenres(works: Work[]): string[] {
  return [...new Set(works.flatMap(work => work.genres ?? []))].sort((a, b) => a.localeCompare(b));
}

/** Candidates for "I don't know what to watch"; a series counts as seen once any episode has progress. */
export function surpriseCandidates(works: Work[], filters: SurpriseFilters, history: Seen[] = []): Work[] {
  const seen = new Set(history.flatMap(item => item.seriesId ? [item.workId, item.seriesId] : [item.workId]));
  return works.filter(work =>
    (filters.kind === 'all' || work.kind === filters.kind)
    && (!filters.genre || (work.genres ?? []).includes(filters.genre))
    && (!filters.unseenOnly || !seen.has(work.id)));
}

/** Random pick that avoids repeating the previous title when another one is available. */
export function pickSurprise(candidates: Work[], previous?: string, random: () => number = Math.random): Work | null {
  const pool = candidates.length > 1 && previous ? candidates.filter(work => work.id !== previous) : candidates;
  return pool.length ? pool[Math.floor(random() * pool.length) % pool.length] : null;
}

/** Logique pure des chapitres et des segments (intro, générique…) de la barre de lecture. */
import type { Media } from './types';

export type Chapter = NonNullable<Media['chapters']>[number];

/** Chapitres valides pour la barre, triés par début. */
export function timelineChapters(chapters: Media['chapters'], duration: number): Chapter[] {
  return (chapters ?? [])
    .filter(chapter => Number.isFinite(chapter.start) && chapter.start >= 0 && chapter.start < duration)
    .toSorted((a, b) => a.start - b.start);
}

/** Dernier chapitre démarré à `time` (liste triée). */
export function chapterAt(chapters: Chapter[], time: number): Chapter | undefined {
  return chapters.findLast(chapter => chapter.start <= time);
}

/** Cible de saut vers le chapitre suivant/précédent ; `null` = ne rien faire. */
export function chapterSkipTarget(
  chapters: Media['chapters'],
  position: number,
  duration: number,
  direction: -1 | 1
): number | null {
  if (!chapters || chapters.length === 0) return null;
  const sorted = [...chapters].sort((a, b) => a.start - b.start);
  if (direction === 1) {
    const next = sorted.find(c => c.start > position + 1);
    if (next) return next.start;
    return duration ? duration : null;
  }
  let currentIndex = -1;
  for (let i = 0; i < sorted.length; i++) {
    if (sorted[i].start <= position + 0.5) currentIndex = i;
    else break;
  }
  if (currentIndex === -1) return 0;
  const current = sorted[currentIndex];
  if (position - current.start > 3) return current.start;
  return currentIndex > 0 ? sorted[currentIndex - 1].start : 0;
}

export interface TimeRange { start: number; end: number }

export function inRange(range: TimeRange | undefined, position: number): boolean {
  return Boolean(range && position >= range.start && position < range.end);
}

/** Position/largeur (%) d'un segment sur la barre. */
export function markerSegmentStyle(range: TimeRange, duration: number): { left: string; width: string } {
  return {
    left: `${Math.max(0, (range.start / duration) * 100)}%`,
    width: `${Math.max(0.5, ((range.end - range.start) / duration) * 100)}%`
  };
}

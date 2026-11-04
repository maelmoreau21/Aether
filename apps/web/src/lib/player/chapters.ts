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

/** Tronçon de la barre de lecture : un chapitre (ou toute la durée sans chapitre). */
export interface TimelineSegment { start: number; end: number; title: string | null }

/**
 * Découpe la barre en tronçons contigus couvrant [0, duration] :
 * un seul tronçon sans titre s'il n'y a aucun chapitre, sinon un par chapitre
 * (précédé d'un tronçon sans titre si le premier chapitre démarre après 1 s).
 * Les chapitres de même début sont fusionnés (le dernier l'emporte, comme `chapterAt`).
 */
export function timelineSegments(chapters: Chapter[], duration: number): TimelineSegment[] {
  if (!Number.isFinite(duration) || duration <= 0) return [];
  if (chapters.length === 0) return [{ start: 0, end: duration, title: null }];
  const starts: Chapter[] = [];
  for (const chapter of chapters) {
    if (starts.length && starts[starts.length - 1].start === chapter.start) starts[starts.length - 1] = chapter;
    else starts.push(chapter);
  }
  const segments: TimelineSegment[] = [];
  if (starts[0].start >= 1) segments.push({ start: 0, end: starts[0].start, title: null });
  starts.forEach((chapter, index) => {
    const start = index === 0 && chapter.start < 1 ? 0 : chapter.start;
    const end = index + 1 < starts.length ? starts[index + 1].start : duration;
    if (end > start) segments.push({ start, end, title: chapter.title || null });
  });
  return segments;
}

/** Index du tronçon contenant `time` (-1 si aucun). */
export function segmentIndexAt(segments: TimelineSegment[], time: number): number {
  return segments.findLastIndex(segment => segment.start <= time);
}

/** Part (0–100 %) du tronçon déjà atteinte à `time`. */
export function segmentFill(segment: TimelineSegment, time: number): number {
  const length = segment.end - segment.start;
  if (length <= 0) return 0;
  return Math.min(100, Math.max(0, ((time - segment.start) / length) * 100));
}

/** Intersection d'une plage (intro, générique…) avec un tronçon, en % du tronçon ; `null` si disjointes. */
export function rangeInSegment(range: TimeRange | undefined, segment: TimelineSegment): { left: number; width: number } | null {
  if (!range) return null;
  const start = Math.max(range.start, segment.start);
  const end = Math.min(range.end, segment.end);
  const length = segment.end - segment.start;
  if (end <= start || length <= 0) return null;
  return { left: ((start - segment.start) / length) * 100, width: ((end - start) / length) * 100 };
}

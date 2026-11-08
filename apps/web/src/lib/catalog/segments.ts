// Episode timeline segments (intro, credits, recap, preview): what the season overview
// endpoint returns, and the rules the editor applies before saving a correction.
// The server repeats every check; these only give immediate, precise feedback.

export const segmentKinds = ['intro', 'credits', 'recap', 'preview'] as const;
export type SegmentKind = typeof segmentKinds[number];

export type TaskState = { status: string; error: string | null; updatedAt: number | null };
export type AutomaticSegment = {
  type: SegmentKind; start: number; end: number; confidence: number;
  extensionId: string; updatedAt: number; usable: boolean;
};
export type ManualSegment = { type: SegmentKind; start: number | null; end: number | null; enabled: boolean; updatedAt: number };
export type SegmentEpisode = {
  assetId: string; workId: string; episodeNumber: number; title: string;
  duration: number | null; minConfidence: number; detectionEnabled: boolean;
  chapters: { start: number; title: string }[];
  fingerprint: TaskState; detection: TaskState;
  automatic: AutomaticSegment[]; manual: ManualSegment[];
};
export type SegmentProvider = { id: string; name: string; version: string; enabled: boolean };
export type SeasonSegments = { seasonId: string; providers: SegmentProvider[]; episodes: SegmentEpisode[] };

export type EffectiveSegment =
  | { source: 'manual'; start: number; end: number }
  | { source: 'automatic'; start: number; end: number; confidence: number; extensionId: string }
  | { source: 'disabled' }
  | { source: 'none' };

/** Largest manual segment the server accepts, in seconds. */
export const MAX_SEGMENT_LENGTH = 600;

/** What the player will use: a manual correction wins, then the best usable detection. */
export function effectiveSegment(episode: SegmentEpisode, kind: SegmentKind): EffectiveSegment {
  const manual = episode.manual.find(item => item.type === kind);
  if (manual) {
    return manual.enabled && manual.start !== null && manual.end !== null
      ? { source: 'manual', start: manual.start, end: manual.end }
      : { source: 'disabled' };
  }
  const automatic = bestDetection(episode, kind, true);
  return automatic
    ? { source: 'automatic', start: automatic.start, end: automatic.end, confidence: automatic.confidence, extensionId: automatic.extensionId }
    : { source: 'none' };
}

/** Most confident detection of a kind; with `usableOnly`, only one the player would accept. */
export function bestDetection(episode: SegmentEpisode, kind: SegmentKind, usableOnly = false): AutomaticSegment | undefined {
  return episode.automatic
    .filter(item => item.type === kind && (!usableOnly || item.usable))
    .sort((a, b) => b.confidence - a.confidence)[0];
}

/**
 * Reads "83", "83.5", "1:23", "1:23.5" or "1:02:03" as seconds. Commas are accepted as the
 * decimal separator. Returns null for anything else, including negative values.
 */
export function parseClock(text: string): number | null {
  const value = text.trim().replace(',', '.');
  if (!value) return null;
  if (/^\d+(\.\d+)?$/.test(value)) return Number(value);
  const match = /^(?:(\d+):)?(\d{1,2}):(\d{1,2}(?:\.\d+)?)$/.exec(value);
  if (!match) return null;
  const [, hours, minutes, seconds] = match;
  if (Number(seconds) >= 60 || (hours !== undefined && Number(minutes) >= 60)) return null;
  return Number(hours ?? 0) * 3600 + Number(minutes) * 60 + Number(seconds);
}

/** "1:23", "1:02:03"; with `precise`, tenths are kept when present ("1:23.5"). */
export function formatClock(seconds: number | null | undefined, precise = false): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '—';
  const rounded = precise ? Math.round(seconds * 10) / 10 : Math.round(seconds);
  const whole = Math.floor(rounded);
  const tenths = Math.round((rounded - whole) * 10);
  const hours = Math.floor(whole / 3600), minutes = Math.floor(whole % 3600 / 60), rest = whole % 60;
  const secondsText = String(rest).padStart(2, '0') + (precise && tenths ? `.${tenths}` : '');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${secondsText}` : `${minutes}:${secondsText}`;
}

export type SegmentProblem = 'segment_invalid_time' | 'segment_end_before_start' | 'segment_beyond_duration' | 'segment_too_long';

/** First problem that would make the server refuse this segment, or null. */
export function segmentProblem(start: number | null, end: number | null, duration: number | null): SegmentProblem | null {
  if (start === null || end === null || !Number.isFinite(start) || !Number.isFinite(end) || start < 0) return 'segment_invalid_time';
  if (end <= start) return 'segment_end_before_start';
  if (duration !== null && end > duration) return 'segment_beyond_duration';
  if (end - start > MAX_SEGMENT_LENGTH) return 'segment_too_long';
  return null;
}

/**
 * The same segment for another episode of the season: credits keep their distance to the end of
 * the file (episodes rarely share a length), the other kinds keep their position. Returns null
 * when the copy does not fit that episode.
 */
export function copySegment(kind: SegmentKind, start: number, end: number, from: number | null, to: number | null): { start: number; end: number } | null {
  let next = { start, end };
  if (kind === 'credits' && from !== null && to !== null) {
    next = { start: to - (from - start), end: to - (from - end) };
  }
  next = { start: Math.max(0, round(next.start)), end: round(next.end) };
  return segmentProblem(next.start, next.end, to) ? null : next;
}

const round = (value: number) => Math.round(value * 1000) / 1000;

/** Episodes covered per kind, and the analysis tasks still waiting or failed. */
export function seasonSummary(episodes: SegmentEpisode[]) {
  const covered = Object.fromEntries(segmentKinds.map(kind => [kind, episodes.filter(episode => {
    const source = effectiveSegment(episode, kind).source;
    return source === 'manual' || source === 'automatic';
  }).length])) as Record<SegmentKind, number>;
  const pending = episodes.filter(episode => ['queued', 'running', 'paused'].includes(episode.fingerprint.status) || ['queued', 'running', 'paused'].includes(episode.detection.status)).length;
  const failed = episodes.filter(episode => episode.fingerprint.status === 'error' || episode.detection.status === 'error').length;
  const weak = episodes.filter(episode => segmentKinds.some(kind => effectiveSegment(episode, kind).source === 'none' && bestDetection(episode, kind))).length;
  return { total: episodes.length, covered, pending, failed, weak };
}

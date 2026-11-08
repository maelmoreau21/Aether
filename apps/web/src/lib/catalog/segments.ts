// Episode timeline segments (intro, credits, recap, preview): what the season overview
// endpoint returns, and the rules the editor applies before saving a correction.
// The server repeats every check; these only give immediate, precise feedback.
import type { TranslationKey } from '../i18n/messages';

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
  /** Reading of the file's chapters (absent from servers before November 2026). */
  chapterReading?: TaskState;
  automatic: AutomaticSegment[]; manual: ManualSegment[];
};
export type SegmentProvider = { id: string; name: string; version: string; enabled: boolean };
export type SeasonSegments = { seasonId: string; providers: SegmentProvider[]; episodes: SegmentEpisode[] };
/** One file (a film or an episode) in the segment editor. */
export type AssetSegments = { kind: 'film' | 'episode'; seasonId: string | null; providers: SegmentProvider[]; asset: SegmentEpisode };

export type EffectiveSegment =
  | { source: 'manual'; start: number; end: number }
  | { source: 'automatic'; start: number; end: number; confidence: number; extensionId: string }
  | { source: 'disabled' }
  | { source: 'none' };

/** Largest manual segment the server accepts, in seconds (film credits may run 20 minutes). */
export const MAX_SEGMENT_LENGTH = 600;
export const MAX_CREDITS_LENGTH = 1200;
export const maxSegmentLength = (kind?: SegmentKind) => kind === 'credits' ? MAX_CREDITS_LENGTH : MAX_SEGMENT_LENGTH;
/** Most files the season overview lists (the server stops there). */
export const SEASON_MARKERS_LIMIT = 200;
/** Shortest gap the editor keeps between the two bounds of a segment, in seconds. */
export const MIN_SEGMENT_GAP = 0.1;

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
 * The times the editor shows for a kind: what the player uses (a manual correction or a usable
 * detection), else the best detection even below the confidence floor, else nothing.
 */
export function suggestedSegment(episode: SegmentEpisode, kind: SegmentKind): { start: number; end: number } | null {
  const shown = effectiveSegment(episode, kind);
  if (shown.source === 'manual' || shown.source === 'automatic') return { start: shown.start, end: shown.end };
  const detected = bestDetection(episode, kind);
  return detected ? { start: detected.start, end: detected.end } : null;
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

/**
 * An end shown to the tenth can read back just past the end of the file (1425.483 s is shown as
 * "23:45.5"): within a tenth of a second, it means the end of the file.
 */
export function fitEnd(end: number | null, duration: number | null): number | null {
  if (end === null || duration === null || end <= duration) return end;
  return end - duration <= MIN_SEGMENT_GAP + 1e-6 ? duration : end;
}

/** A dragged bound, kept at least MIN_SEGMENT_GAP before (start) or after (end) the other one. */
export function clampBound(field: 'start' | 'end', value: number, other: number | null): number {
  if (other === null || !Number.isFinite(other)) return Math.max(0, value);
  return field === 'start' ? Math.max(0, Math.min(value, other - MIN_SEGMENT_GAP)) : Math.max(value, other + MIN_SEGMENT_GAP);
}

export type SegmentProblem ='segment_invalid_time' | 'segment_end_before_start' | 'segment_beyond_duration' | 'segment_too_long' | 'segment_too_long_credits';

/** First problem that would make the server refuse this segment, or null. */
export function segmentProblem(start: number | null, end: number | null, duration: number | null, kind?: SegmentKind): SegmentProblem | null {
  if (start === null || end === null || !Number.isFinite(start) || !Number.isFinite(end) || start < 0) return 'segment_invalid_time';
  if (end <= start) return 'segment_end_before_start';
  if (duration !== null && end > duration) return 'segment_beyond_duration';
  if (end - start > maxSegmentLength(kind)) return kind === 'credits' ? 'segment_too_long_credits' : 'segment_too_long';
  return null;
}

/** The point of `points` within `window` seconds of `time`, else `time` (timeline snapping). */
export function snapTo(points: number[], time: number, window: number): number {
  let best = time, distance = window;
  for (const point of points) {
    const gap = Math.abs(point - time);
    if (gap <= distance) { best = point; distance = gap; }
  }
  return best;
}

/** Label of one analysis task's status. */
export function taskLabel(status: string): TranslationKey {
  switch (status) {
    case 'queued': return 'segment_task_queued';
    case 'running': return 'segment_task_running';
    case 'paused': return 'segment_task_paused';
    case 'completed': return 'segment_task_completed';
    case 'error': return 'segment_task_error';
    case 'cancelled': return 'segment_task_cancelled';
    default: return 'segment_task_missing';
  }
}

const activeStatuses = ['queued', 'running', 'paused'];
const missingTask: TaskState = { status: 'missing', error: null, updatedAt: null };
/** Confidence of a segment taken from a named chapter: such a segment needs no audio check. */
export const NAMED_CHAPTER_CONFIDENCE = 0.98;

/** The named chapters already gave an intro and end credits, so the audio is not compared. */
export function chaptersSettled(episode: SegmentEpisode): boolean {
  return (['intro', 'credits'] as const).every(kind => (bestDetection(episode, kind)?.confidence ?? 0) >= NAMED_CHAPTER_CONFIDENCE);
}

/** Where the analysis of a file stands, from its three tasks. */
export type AnalysisState = { key: TranslationKey; tone: 'ok' | 'wait' | 'err' | 'mute'; detail?: string };
export function analysisState(episode: SegmentEpisode, film = false): AnalysisState {
  if (!episode.detectionEnabled) return { key: 'segment_status_detection_off', tone: 'mute' };
  const chapters = episode.chapterReading ?? missingTask;
  const tasks = film ? [chapters] : [chapters, episode.fingerprint, episode.detection];
  const failed = tasks.find(task => task.status === 'error');
  if (!film && episode.detection.status === 'error') return { key: 'segment_status_detection_failed', tone: 'err', detail: episode.detection.error ?? undefined };
  if (!film && episode.fingerprint.status === 'error') return { key: 'segment_status_audio_failed', tone: 'err', detail: episode.fingerprint.error ?? undefined };
  if (failed) return { key: 'segment_status_chapters_failed', tone: 'err', detail: failed.error ?? undefined };
  if (tasks.some(task => ['queued', 'paused'].includes(task.status))) return { key: 'segment_status_queued', tone: 'wait' };
  if (tasks.some(task => task.status === 'running')) return { key: 'segment_status_running', tone: 'wait' };
  // The audio is fingerprinted but no comparison ran: the season does not have enough other
  // fingerprinted episodes yet (they may still be queued, or missing).
  if (!film && episode.fingerprint.status === 'completed' && episode.detection.status === 'missing' && !chaptersSettled(episode)) {
    return { key: 'segment_status_waiting_peers', tone: 'wait' };
  }
  if (tasks.some(task => task.status === 'completed')) return { key: 'segment_status_analysed', tone: 'ok' };
  return { key: 'segment_status_not_analysed', tone: 'mute' };
}

/** One step of a file's analysis, as the editor lists it. */
export type AnalysisStep = { step: 'chapters' | 'fingerprint' | 'detection'; task: TaskState; state: TranslationKey; tone: AnalysisState['tone'] };
export function analysisSteps(episode: SegmentEpisode, film = false): AnalysisStep[] {
  const step = (name: AnalysisStep['step'], task: TaskState): AnalysisStep => {
    if (name !== 'chapters' && task.status === 'missing') {
      if (chaptersSettled(episode)) return { step: name, task, state: 'segment_task_not_needed', tone: 'ok' };
      if (name === 'detection' && episode.fingerprint.status === 'completed') return { step: name, task, state: 'segment_task_waiting_peers', tone: 'wait' };
    }
    const tone = task.status === 'completed' ? 'ok' : task.status === 'error' ? 'err' : activeStatuses.includes(task.status) ? 'wait' : 'mute';
    return { step: name, task, state: taskLabel(task.status), tone };
  };
  const chapters = step('chapters', episode.chapterReading ?? missingTask);
  return film ? [chapters] : [chapters, step('fingerprint', episode.fingerprint), step('detection', episode.detection)];
}

/** Whether a task of the file's analysis is still waiting or running (the editor then refreshes). */
export function analysisActive(episode: SegmentEpisode, film = false): boolean {
  const chapters = episode.chapterReading ?? missingTask;
  const tasks = film ? [chapters] : [chapters, episode.fingerprint, episode.detection];
  return tasks.some(task => activeStatuses.includes(task.status));
}

/**
 * Runs `refresh` every `every` ms for as long as it resolves true, and for `max` ms at most.
 * Returns the function that stops it (call it when the view goes away).
 */
export function pollWhile(refresh: () => Promise<boolean>, every = 4000, max = 120_000): () => void {
  const until = Date.now() + max;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const tick = async () => {
    if (stopped) return;
    let again: boolean;
    try { again = await refresh(); } catch { again = true; }
    if (!stopped && again && Date.now() + every <= until) timer = setTimeout(tick, every);
  };
  timer = setTimeout(tick, every);
  return () => { stopped = true; clearTimeout(timer); };
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
  // Rounding must not push credits that end with the file past its end.
  next = { start: Math.max(0, round(next.start)), end: to === null ? round(next.end) : Math.min(round(next.end), to) };
  return segmentProblem(next.start, next.end, to, kind) ? null : next;
}

const round = (value: number) => Math.round(value * 1000) / 1000;

/**
 * What « apply to the whole season » writes: one copy per other episode, except the episodes the
 * copy does not fit (or whose length is unknown) and, unless `replaceManual`, the episodes that
 * already have their own correction of that kind.
 */
export type CopyPlan = { copies: { assetId: string; start: number; end: number }[]; unfit: number; kept: number };
export function planCopies(kind: SegmentKind, start: number, end: number, source: SegmentEpisode, siblings: SegmentEpisode[], replaceManual = false): CopyPlan {
  const plan: CopyPlan = { copies: [], unfit: 0, kept: 0 };
  for (const other of siblings) {
    if (other.assetId === source.assetId) continue;
    if (!replaceManual && other.manual.some(item => item.type === kind)) { plan.kept++; continue; }
    const copy = other.duration === null ? null : copySegment(kind, start, end, source.duration, other.duration);
    if (copy) plan.copies.push({ assetId: other.assetId, ...copy });
    else plan.unfit++;
  }
  return plan;
}

/** Episodes covered per kind, and the analysis tasks still waiting or failed. */
export function seasonSummary(episodes: SegmentEpisode[]) {
  const covered = Object.fromEntries(segmentKinds.map(kind => [kind, episodes.filter(episode => {
    const source = effectiveSegment(episode, kind).source;
    return source === 'manual' || source === 'automatic';
  }).length])) as Record<SegmentKind, number>;
  const tasks = (episode: SegmentEpisode) => [episode.chapterReading ?? missingTask, episode.fingerprint, episode.detection];
  const pending = episodes.filter(episode => tasks(episode).some(task => activeStatuses.includes(task.status))).length;
  const failed = episodes.filter(episode => tasks(episode).some(task => task.status === 'error')).length;
  const weak = episodes.filter(episode => segmentKinds.some(kind => effectiveSegment(episode, kind).source === 'none' && bestDetection(episode, kind))).length;
  return { total: episodes.length, covered, pending, failed, weak };
}

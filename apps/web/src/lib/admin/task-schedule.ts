import type { TranslationKey, TranslationParams } from '../i18n';

/** Shapes of the unified `/api/v1/admin/tasks` API. */
export type TriggerType = 'daily' | 'weekly' | 'interval' | 'startup';
export interface TaskTrigger {
  id: string;
  type: TriggerType;
  timeOfDayMinutes: number | null;
  dayOfWeek: number | null;
  intervalMinutes: number | null;
  maxRuntimeSeconds: number | null;
  enabled: boolean;
  nextFireAt: number | null;
}
export interface TaskLastRun {
  start: number | null;
  end: number | null;
  outcome: string;
  message: string | null;
  errorKind: string | null;
  errorScope: string | null;
}
/** Counts of a library media sub-task run (files, or works for images). */
export interface TaskStats {
  done: number;
  failed: number;
  pending: number;
  /** Images downloaded, for the image sub-task. */
  images: number | null;
}
/** Media sub-tasks of a library: shown under its scan, run by hand or after each scan. */
export type MediaTaskKind = 'trickplay' | 'intros' | 'artwork';
export interface ScheduledTask {
  id: string;
  kind: 'system' | 'scan' | 'backup' | MediaTaskKind;
  name: string;
  description: string;
  category: string;
  state: 'idle' | 'queued' | 'running';
  progress: number | null;
  nextRunAt: number | null;
  lastRun: TaskLastRun | null;
  reason: string | null;
  libraryId: string | null;
  libraryKind: string | null;
  warningsCount: number;
  issues: string[];
  triggers: TaskTrigger[];
  /** The library scan a media sub-task belongs to (`scan-<library>`). */
  parentId?: string | null;
  stats?: TaskStats | null;
  /** For a media sub-task: whether it also runs on its own after each library scan. */
  afterScan?: boolean | null;
}
export interface TriggerDraft {
  type: TriggerType;
  dayOfWeek: number;
  timeOfDayMinutes: number;
  intervalMinutes: number;
  /** Optional time limit in hours; empty or 0 means none. */
  maxRuntimeHours: number | null;
}

type Translate = (key: TranslationKey, params?: TranslationParams) => string;

/** Interval choices offered by the trigger dialog, in minutes. */
export const intervalOptions = [15, 30, 60, 120, 180, 240, 360, 480, 720, 1440] as const;
/** Time-of-day choices: every 30 minutes. */
export const timeSlots = Array.from({ length: 48 }, (_, index) => index * 30);
/** Monday first, as administrators read a week; values are Sunday-based (0 = Sunday). */
export const weekdays = [1, 2, 3, 4, 5, 6, 0] as const;

const dayKeys: TranslationKey[] = ['sched_day_0', 'sched_day_1', 'sched_day_2', 'sched_day_3', 'sched_day_4', 'sched_day_5', 'sched_day_6'];

export function dayLabel(day: number, t: Translate): string {
  return t(dayKeys[((day % 7) + 7) % 7]);
}

export function clockLabel(minutes: number): string {
  const safe = Math.max(0, Math.min(1439, Math.floor(minutes)));
  return `${String(Math.floor(safe / 60)).padStart(2, '0')}:${String(safe % 60).padStart(2, '0')}`;
}

export function intervalLabel(minutes: number, t: Translate): string {
  if (minutes % 60 === 0) {
    const hours = minutes / 60;
    return hours === 1 ? t('sched_every_hour') : t('sched_every_hours', { count: hours });
  }
  return t('sched_every_minutes', { count: minutes });
}

/** "Tous les jours à 05:00", "Chaque lundi à 03:00", "Toutes les 12 heures", "Au démarrage du serveur". */
export function triggerLabel(trigger: Pick<TaskTrigger, 'type' | 'timeOfDayMinutes' | 'dayOfWeek' | 'intervalMinutes'>, t: Translate): string {
  switch (trigger.type) {
    case 'daily':
      return t('sched_daily_at', { time: clockLabel(trigger.timeOfDayMinutes ?? 0) });
    case 'weekly':
      return t('sched_weekly_at', { day: dayLabel(trigger.dayOfWeek ?? 0, t), time: clockLabel(trigger.timeOfDayMinutes ?? 0) });
    case 'interval':
      return intervalLabel(trigger.intervalMinutes ?? 1440, t);
    default:
      return t('sched_at_startup');
  }
}

/** "45 s", "3 min 20 s", "2 h 05 min". */
export function durationLabel(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  if (total < 60) return `${total} s`;
  if (total < 3600) {
    const rest = total % 60;
    return rest ? `${Math.floor(total / 60)} min ${rest} s` : `${total / 60} min`;
  }
  const minutes = Math.floor((total % 3600) / 60);
  return `${Math.floor(total / 3600)} h ${String(minutes).padStart(2, '0')} min`;
}

export function agoLabel(timestamp: number, now: number, t: Translate): string {
  const elapsed = Math.max(0, now - timestamp);
  if (elapsed < 60) return t('sched_ago_now');
  if (elapsed < 3600) return t('sched_ago_minutes', { count: Math.floor(elapsed / 60) });
  if (elapsed < 86400) return t('sched_ago_hours', { count: Math.floor(elapsed / 3600) });
  return t('sched_ago_days', { count: Math.floor(elapsed / 86400) });
}

export type SummaryTone = 'ok' | 'error' | 'warning' | 'muted' | 'active';

/** One-line status of a task row, Jellyfin style. */
export function lastRunSummary(task: Pick<ScheduledTask, 'state' | 'progress' | 'lastRun'>, now: number, t: Translate): { text: string; tone: SummaryTone } {
  if (task.state === 'running') {
    return { text: task.progress !== null && task.progress > 0 ? t('sched_running_progress', { progress: task.progress }) : t('sched_running'), tone: 'active' };
  }
  if (task.state === 'queued') return { text: t('sched_queued'), tone: 'active' };
  const run = task.lastRun;
  if (!run) return { text: t('sched_never'), tone: 'muted' };
  const when = run.end ?? run.start;
  if (run.outcome === 'failed') {
    return { text: run.message ? t('sched_failed', { message: run.message }) : t('sched_failed_plain'), tone: 'error' };
  }
  if (run.outcome === 'interrupted' || run.outcome === 'cancelled') {
    const key = run.outcome === 'interrupted' ? 'sched_interrupted' : 'sched_cancelled';
    return { text: t(key, { ago: when ? agoLabel(when, now, t) : '' }).trim(), tone: 'warning' };
  }
  if (!when) return { text: t('sched_never'), tone: 'muted' };
  const ago = agoLabel(when, now, t);
  if (run.start && run.end && run.end >= run.start) return { text: t('sched_last_run', { ago, duration: durationLabel(run.end - run.start) }), tone: 'ok' };
  return { text: t('sched_last_run_short', { ago }), tone: 'ok' };
}

export type TaskStatus = 'running' | 'failed' | 'warning' | 'ok' | 'never';

/** Status of a task at a glance: drives the row icon and the "failed" filter. */
export function taskStatus(task: Pick<ScheduledTask, 'state' | 'lastRun'>): TaskStatus {
  if (task.state !== 'idle') return 'running';
  const outcome = task.lastRun?.outcome;
  if (!outcome || !(task.lastRun?.end ?? task.lastRun?.start)) return 'never';
  if (outcome === 'failed') return 'failed';
  if (outcome === 'interrupted' || outcome === 'cancelled') return 'warning';
  return 'ok';
}

/** One line for the list: the first trigger, "+N" for the others, or manual only. */
export function scheduleSummary(triggers: Pick<TaskTrigger, 'type' | 'timeOfDayMinutes' | 'dayOfWeek' | 'intervalMinutes' | 'enabled'>[], t: Translate): string {
  const enabled = triggers.filter(trigger => trigger.enabled);
  if (!enabled.length) return t('sched_manual_only');
  const first = triggerLabel(enabled[0], t);
  return enabled.length > 1 ? t('sched_more_triggers', { first, count: enabled.length - 1 }) : first;
}

/** Request body of `POST /api/v1/admin/tasks/{id}/triggers`. */
export function triggerPayload(draft: TriggerDraft): Record<string, string | number> {
  const body: Record<string, string | number> = { type: draft.type };
  if (draft.type === 'daily' || draft.type === 'weekly') body.timeOfDayMinutes = draft.timeOfDayMinutes;
  if (draft.type === 'weekly') body.dayOfWeek = draft.dayOfWeek;
  if (draft.type === 'interval') body.intervalMinutes = draft.intervalMinutes;
  if (draft.maxRuntimeHours && draft.maxRuntimeHours > 0) body.maxRuntimeSeconds = Math.max(60, Math.round(draft.maxRuntimeHours * 3600));
  return body;
}

const categoryKeys: Record<string, TranslationKey> = {
  'Bibliothèques': 'sched_category_libraries',
  Maintenance: 'sched_category_maintenance',
  Extensions: 'sched_category_extensions'
};
/** Libraries first, then maintenance, extensions and anything else alphabetically. */
export function categoryOrder(category: string): number {
  const index = Object.keys(categoryKeys).indexOf(category);
  return index < 0 ? Object.keys(categoryKeys).length : index;
}
export function categoryLabel(category: string, t: Translate): string {
  const key = categoryKeys[category];
  return key ? t(key) : category;
}

const mediaNameKeys: Record<MediaTaskKind, TranslationKey> = {
  trickplay: 'sched_trickplay_name',
  intros: 'sched_intros_name',
  artwork: 'sched_artwork_name'
};

export function isMediaTask(task: Pick<ScheduledTask, 'kind'>): task is Pick<ScheduledTask, 'kind'> & { kind: MediaTaskKind } {
  return task.kind in mediaNameKeys;
}

export function taskName(task: Pick<ScheduledTask, 'kind' | 'name' | 'libraryId'>, libraryName: string | undefined, t: Translate): string {
  if (task.kind === 'backup') return t('sched_backup_name');
  if (task.kind === 'scan' && libraryName) return t('sched_scan_name', { name: libraryName });
  if (isMediaTask(task) && libraryName) return t(mediaNameKeys[task.kind], { name: libraryName });
  return task.name;
}

/** "12 terminés · 3 en échec · 25 en attente", or works and images for the image sub-task. */
export function statsLabel(task: Pick<ScheduledTask, 'kind' | 'stats'>, t: Translate): string {
  const stats = task.stats;
  if (!stats) return '';
  const parts = task.kind === 'artwork'
    ? [t('sched_stats_works', { count: stats.done }), t('sched_stats_images', { count: stats.images ?? 0 })]
    : [t('sched_stats_done', { count: stats.done })];
  if (stats.failed) parts.push(t('sched_stats_failed', { count: stats.failed }));
  if (stats.pending) parts.push(t('sched_stats_pending', { count: stats.pending }));
  return parts.join(' · ');
}

/** Top-level tasks in order, each followed by its media sub-tasks. A sub-task whose parent is
 * not listed (filtered out) is shown on its own. */
export function nestTasks<T extends Pick<ScheduledTask, 'id' | 'parentId'>>(tasks: T[]): { task: T; children: T[] }[] {
  const ids = new Set(tasks.map(task => task.id));
  return tasks
    .filter(task => !task.parentId || !ids.has(task.parentId))
    .map(task => ({ task, children: tasks.filter(child => child.parentId === task.id) }));
}

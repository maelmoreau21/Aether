import { get } from 'svelte/store';
import { session, getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import { serverPresentation } from '../config/presentation';
import type { PlaybackState } from './types';

import { tr } from '../i18n';
export interface Progress { workId: string; position: number; duration: number; watched: boolean; revision: number; title?: string; kind?: string; seriesId?: string }

const warmed = new Map<string, { at: number; value: Promise<Progress> }>();
/** Writes still on the wire: a read issued meanwhile would return the revision they are replacing. */
const inflight = new Map<string, Promise<unknown>>();
onSessionGenerationChange(() => warmed.clear());

/** Read ahead on a play gesture's approach, never across accounts or progress writes. */
export function preloadProgress(work: string): void {
  if (!get(session)) return;
  const existing = warmed.get(work);
  if (existing && Date.now() - existing.at < 5000) return;
  const oldest = warmed.keys().next().value;
  if (warmed.size >= 32 && oldest !== undefined) warmed.delete(oldest);
  const value = fetchProgress(work);
  const entry = { at: Date.now(), value };
  warmed.set(work, entry);
  void value.catch(() => { if (warmed.get(work) === entry) warmed.delete(work); });
}

export async function readProgress(work: string): Promise<Progress> {
  const ready = warmed.get(work);
  warmed.delete(work);
  if (ready && Date.now() - ready.at < 5000) return ready.value;
  return fetchProgress(work);
}

async function fetchProgress(work: string): Promise<Progress> {
  const pending = inflight.get(work);
  if (pending) await pending.catch(() => undefined);
  const response = await fetch(`/api/v1/progress/${encodeURIComponent(work)}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(2500) });
  if (!response.ok) throw new Error(tr('resume_unavailable_progress_not_loaded'));
  return response.json();
}

/**
 * A progress write that failed. `transient` failures (slow server, network blip, 5xx) are retried by
 * the next periodic save; the others (conflict, rights, session) stop the tracker.
 */
export class ProgressWriteError extends Error {
  constructor(message: string, readonly transient: boolean, readonly conflict = false) {
    super(message);
    this.name = 'ProgressWriteError';
  }
}

export function writeProgress(progress: Progress, keepalive = false): Promise<Progress> {
  warmed.delete(progress.workId);
  const task = sendProgress(progress, keepalive);
  inflight.set(progress.workId, task);
  const forget = () => { if (inflight.get(progress.workId) === task) inflight.delete(progress.workId); };
  void task.then(forget, forget);
  return task;
}

async function sendProgress(progress: Progress, keepalive: boolean): Promise<Progress> {
  const current = get(session);
  if (!current) throw new ProgressWriteError(tr('session_required_to_save_progress'), false);
  let response: Response;
  try {
    response = await fetch(`/api/v1/progress/${encodeURIComponent(progress.workId)}`, {
      method: 'PUT', credentials: 'same-origin', keepalive, signal: AbortSignal.timeout(10000),
      headers: { 'content-type': 'application/json', 'x-csrf-token': current.csrfToken },
      body: JSON.stringify({ position: progress.position, duration: progress.duration, watched: progress.watched, expectedRevision: progress.revision })
    });
  } catch {
    // Timeout ("signal timed out") or network failure: never shown raw, retried by the next save.
    throw new ProgressWriteError(tr('progress_save_delayed'), true);
  }
  if (response.status === 409) throw new ProgressWriteError(tr('progress_changed_in_another_session_close'), false, true);
  if (response.status >= 500 || response.status === 429 || response.status === 408) throw new ProgressWriteError(tr('progress_save_delayed'), true);
  if (!response.ok) throw new ProgressWriteError(tr('progress_not_saved_check_your_connection'), false);
  const saved: Progress = await response.json();
  window.dispatchEvent(new Event('media-progress-updated'));
  return saved;
}

export function isResumable(progress: Progress, cfg?: { minResumePct?: number; maxResumePct?: number; minResumeDurationSeconds?: number }): boolean {
  if (progress.watched || progress.position <= 0) return false;
  const minPct = cfg?.minResumePct ?? 0;
  const maxPct = cfg?.maxResumePct ?? 90;
  const minDuration = cfg?.minResumeDurationSeconds ?? 0;
  if (progress.duration > 0) {
    if (progress.duration < minDuration) return false;
    const ratio = (progress.position / progress.duration) * 100;
    if (ratio < minPct || ratio >= maxPct) return false;
  }
  return true;
}

export function isCreditsOrCompleted(position: number, duration: number, creditsStart?: number, maxResumePct = 90): boolean {
  if (!Number.isFinite(duration) || duration <= 0 || position <= 0) return false;
  if (creditsStart !== undefined && Number.isFinite(creditsStart) && creditsStart > 0 && creditsStart < duration && position >= creditsStart) {
    return true;
  }
  const ratio = position / duration;
  const heuristics = maxResumePct <= 90;
  if (heuristics && duration >= 3600 && (ratio >= 0.88 || (ratio >= 0.85 && duration - position <= 480))) {
    return true;
  }
  if (heuristics && duration >= 1200 && ratio >= 0.88) {
    return true;
  }
  return ratio >= maxResumePct / 100;
}

export class ProgressTracker {
  private revision = 0;
  private generation = getSessionGeneration();
  private queue = Promise.resolve();
  private lastSave = Date.now();
  private failed = false;
  private watched = false;
  private creditsStart?: number;
  private transientFailures = 0;
  static readonly TRANSIENT_FAILURES_BEFORE_NOTICE = 3;
  /**
   * `shared`: playback is driven by a watch party. Several windows of the same account then write the
   * same work's progress at the same time; the last position wins instead of stopping the tracker.
   */
  constructor(private work: string, private onError: (message: string) => void, creditsStart?: number, private shared = false) {
    this.creditsStart = creditsStart;
  }
  setCreditsStart(creditsStart?: number) {
    this.creditsStart = creditsStart;
  }
  async load(): Promise<number> {
    try {
      const progress = await readProgress(this.work);
      if (this.generation !== getSessionGeneration()) { this.failed = true; return 0; }
      this.revision = progress.revision; this.watched = progress.watched;
      const cfg = get(serverPresentation);
      if (!isResumable(progress, cfg)) return 0;
      return progress.position;
    } catch (error) { this.fail(error instanceof DOMException && error.name==='TimeoutError' ? new Error(tr('resume_unavailable_after_2_5_seconds')) : error); return 0; }
  }
  private fail(error: unknown) { this.failed = true; this.onError(error instanceof Error ? error.message : tr('progress_unavailable')); }
  save(state: PlaybackState, force = false, ended = false) {
    if (this.failed || this.generation !== getSessionGeneration() || !Number.isFinite(state.duration) || state.duration <= 0 || (state.position <= 0 && !force)) return;
    if (!force && Date.now() - this.lastSave < 5000) return;
    this.lastSave = Date.now();
    const position = Math.max(0, Math.min(state.position, state.duration));
    const duration = state.duration;
    const watched = this.watched || ended || isCreditsOrCompleted(position, duration, this.creditsStart, get(serverPresentation).maxResumePct);
    this.queue = this.queue.then(async () => {
      if (this.failed || this.generation !== getSessionGeneration()) return;
      try {
        let progress: Progress;
        try {
          progress = await writeProgress({ workId: this.work, position, duration, watched, revision: this.revision }, force);
        } catch (error) {
          if (!this.shared || !(error instanceof ProgressWriteError) || !error.conflict) throw error;
          const latest = await fetchProgress(this.work).catch(() => { throw new ProgressWriteError(tr('progress_save_delayed'), true); });
          if (this.failed || this.generation !== getSessionGeneration()) return;
          progress = await writeProgress({ workId: this.work, position, duration, watched: watched || latest.watched, revision: latest.revision }, force);
        }
        this.revision = progress.revision; this.watched = progress.watched;
        this.transientFailures = 0;
      } catch (error) {
        if (!(error instanceof ProgressWriteError) || !error.transient) { this.fail(error); return; }
        // One slow answer must not stop saving for the rest of the session: the next periodic save
        // retries. Tell the viewer only when the server keeps failing.
        this.transientFailures += 1;
        if (this.transientFailures === ProgressTracker.TRANSIENT_FAILURES_BEFORE_NOTICE) this.onError(error.message);
      }
    });
  }
}

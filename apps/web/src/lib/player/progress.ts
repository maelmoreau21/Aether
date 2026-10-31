import { get } from 'svelte/store';
import { session, getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import type { PlaybackState } from './types';

export interface Progress { workId: string; position: number; duration: number; watched: boolean; revision: number; title?: string; kind?: string; seriesId?: string }

const warmed = new Map<string, { at: number; value: Promise<Progress> }>();
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
  const response = await fetch(`/api/v1/progress/${encodeURIComponent(work)}`, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(2500) });
  if (!response.ok) throw new Error('Reprise indisponible : progression non chargée.');
  return response.json();
}

export async function writeProgress(progress: Progress, keepalive = false): Promise<Progress> {
  warmed.delete(progress.workId);
  const current = get(session);
  if (!current) throw new Error('Session requise pour enregistrer la progression.');
  const response = await fetch(`/api/v1/progress/${encodeURIComponent(progress.workId)}`, {
    method: 'PUT', credentials: 'same-origin', keepalive, signal: AbortSignal.timeout(10000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current.csrfToken },
    body: JSON.stringify({ position: progress.position, duration: progress.duration, watched: progress.watched, expectedRevision: progress.revision })
  });
  if (response.status === 409) throw new Error('La progression a changé dans une autre session. Fermez puis rouvrez le lecteur pour la synchroniser.');
  if (!response.ok) throw new Error('Progression non enregistrée. Vérifiez votre connexion et vos droits.');
  const saved: Progress = await response.json();
  window.dispatchEvent(new Event('media-progress-updated'));
  return saved;
}

export function isCreditsOrCompleted(position: number, duration: number, creditsStart?: number): boolean {
  if (!Number.isFinite(duration) || duration <= 0 || position <= 0) return false;
  if (creditsStart !== undefined && Number.isFinite(creditsStart) && creditsStart > 0 && creditsStart < duration && position >= creditsStart) {
    return true;
  }
  const ratio = position / duration;
  if (duration >= 3600 && (ratio >= 0.88 || (ratio >= 0.85 && duration - position <= 480))) {
    return true;
  }
  if (duration >= 1200 && ratio >= 0.88) {
    return true;
  }
  return ratio >= 0.9;
}

export class ProgressTracker {
  private revision = 0;
  private generation = getSessionGeneration();
  private queue = Promise.resolve();
  private lastSave = Date.now();
  private failed = false;
  private watched = false;
  private creditsStart?: number;
  constructor(private work: string, private onError: (message: string) => void, creditsStart?: number) {
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
      return progress.watched ? 0 : progress.position;
    } catch (error) { this.fail(error instanceof DOMException && error.name==='TimeoutError' ? new Error('Reprise indisponible après 2,5 secondes. Lecture depuis le début ; progression conservée sur le serveur.') : error); return 0; }
  }
  private fail(error: unknown) { this.failed = true; this.onError(error instanceof Error ? error.message : 'Progression indisponible.'); }
  save(state: PlaybackState, force = false, ended = false) {
    if (this.failed || this.generation !== getSessionGeneration() || !Number.isFinite(state.duration) || state.duration <= 0 || (state.position <= 0 && !force)) return;
    if (!force && Date.now() - this.lastSave < 5000) return;
    this.lastSave = Date.now();
    const position = Math.max(0, Math.min(state.position, state.duration));
    const duration = state.duration;
    const watched = this.watched || ended || isCreditsOrCompleted(position, duration, this.creditsStart);
    this.queue = this.queue.then(async () => {
      if (this.failed || this.generation !== getSessionGeneration()) return;
      try {
        const progress = await writeProgress({ workId: this.work, position, duration, watched, revision: this.revision }, force);
        this.revision = progress.revision; this.watched = progress.watched;
      } catch (error) { this.fail(error); }
    });
  }
}

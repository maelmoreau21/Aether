import { writable } from 'svelte/store';
import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import { type Progress, writeProgress, readProgress } from '../player/progress';

export const history = writable<Progress[]>([]);
let pending: Promise<void> | undefined;
let loaded = 0;
onSessionGenerationChange(() => { history.set([]); pending = undefined; loaded = 0; });
if (typeof window !== 'undefined') window.addEventListener('media-progress-updated', () => { void loadHistory(true).catch(() => {}); });
export function loadHistory(force = false): Promise<void> {
  if (pending) return force ? pending.then(() => loadHistory(true)) : pending;
  if (!force && Date.now() - loaded < 30000) return Promise.resolve();
  const generation = getSessionGeneration();
  const operation = (async () => {
    const response = await fetch('/api/v1/progress', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error('Historique indisponible.');
    const result: { items: Progress[] } = await response.json();
    if (generation === getSessionGeneration()) { history.set(result.items); loaded = Date.now(); }
  })();
  pending = operation;
  void operation.finally(() => { if (pending === operation) pending = undefined; }).catch(() => {});
  return operation;
}
export async function markWatched(workId: string, watched: boolean, fallbackDuration?: number) {
  const generation = getSessionGeneration();
  for (let attempt = 0; attempt < 3; attempt++) {
    const progress = await readProgress(workId);
    if (generation !== getSessionGeneration()) return;
    const duration = progress.duration > 0
      ? progress.duration
      : (fallbackDuration && Number.isFinite(fallbackDuration) && fallbackDuration > 0 ? fallbackDuration : 60);
    try {
      await writeProgress({ ...progress, duration, watched, position: watched ? duration : 0 });
      if (generation === getSessionGeneration()) await loadHistory(true);
      return;
    } catch (err) {
      if (attempt < 2 && err instanceof Error && err.message.includes('La progression a changé')) {
        await new Promise(r => setTimeout(r, 100));
        continue;
      }
      throw err;
    }
  }
}

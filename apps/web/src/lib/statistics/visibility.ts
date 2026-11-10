// Whether the signed-in account may read statistics (collection on, and personal
// consultation allowed or administrator). Cached per session generation.
import { derived, readable, type Readable } from 'svelte/store';
import { getSessionGeneration, onSessionGenerationChange, session } from '../auth/session';

let cached: { generation: number; value: Promise<boolean> } | undefined;

export function canViewStatistics(): Promise<boolean> {
  const generation = getSessionGeneration();
  if (cached?.generation === generation) return cached.value;
  const value = fetch('/api/v1/playback/statistics', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(5000) })
    .then(response => response.ok ? response.json() : null)
    .then(body => body?.canView === true)
    .catch(() => false);
  cached = { generation, value };
  return value;
}

/** Store form, refreshed on sign-in changes and when an administrator saves the settings. */
export const statisticsVisible: Readable<boolean> = readable(false, set => {
  let disposed = false;
  const refresh = () => { cached = undefined; void canViewStatistics().then(value => { if (!disposed) set(value); }); };
  refresh();
  const off = onSessionGenerationChange(refresh);
  if (typeof window !== 'undefined') window.addEventListener('statistics-settings-changed', refresh);
  return () => { disposed = true; off(); if (typeof window !== 'undefined') window.removeEventListener('statistics-settings-changed', refresh); };
});

/** The audience tab of a title (film or series page): administrators only, never regular users. */
export const titleStatisticsVisible: Readable<boolean> = derived(
  [statisticsVisible, session],
  ([visible, account]) => visible && account?.user.role === 'administrator'
);

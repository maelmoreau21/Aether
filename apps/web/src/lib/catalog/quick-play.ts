import { getContext, setContext } from 'svelte';
import type { Work } from './api';

/** Lets every poster card start playback through the catalogue's own launcher. */
const KEY = Symbol('quick-play');
export type QuickPlay = (work: Work) => Promise<void>;
export function provideQuickPlay(play: QuickPlay): void { setContext(KEY, play); }
export function quickPlay(): QuickPlay | undefined { return getContext<QuickPlay | undefined>(KEY); }

type Feedback = { favorite: boolean; rating: number | null };
/** Toggles the private favorite flag while keeping the private rating unchanged. */
export async function toggleFavorite(workId: string, csrf: string): Promise<boolean> {
  const url = `/api/v1/feedback/${encodeURIComponent(workId)}`;
  const current = await fetch(url, { cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (!current.ok) throw new Error('Favori indisponible.');
  const value: Feedback = await current.json();
  const saved = await fetch(url, {
    method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
    body: JSON.stringify({ ...value, favorite: !value.favorite }), signal: AbortSignal.timeout(10000)
  });
  if (!saved.ok) throw new Error('Favori non enregistré.');
  return (await saved.json() as Feedback).favorite;
}
export async function readFavorite(workId: string): Promise<boolean> {
  const response = await fetch(`/api/v1/feedback/${encodeURIComponent(workId)}`, { cache: 'no-store', signal: AbortSignal.timeout(10000) });
  return response.ok ? Boolean((await response.json() as Feedback).favorite) : false;
}

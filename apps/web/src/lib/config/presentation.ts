import { writable } from 'svelte/store';
export const serverPresentation = writable({ serverName: '', welcomeMessage: '' });
export async function loadServerPresentation() {
  try {
    const response = await fetch('/api/v1/server', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (response.ok) serverPresentation.set(await response.json());
  } catch { /* Optional presentation never blocks authentication or playback. */ }
}

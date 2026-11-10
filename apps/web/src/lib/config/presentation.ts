import { derived, writable } from 'svelte/store';
export const serverPresentation = writable<{
  serverName: string;
  welcomeMessage: string;
  minResumePct?: number;
  maxResumePct?: number;
  minResumeDurationSeconds?: number;
  activityLogRetentionDays?: number;
  corsHosts?: string[];
  watchTogetherEnabled?: boolean;
  remoteControlEnabled?: boolean;
}>({ serverName: '', welcomeMessage: '' });
/** Off only when the server says so: an older Core without the setting keeps the feature. */
export const watchTogetherEnabled = derived(serverPresentation, value => value.watchTogetherEnabled !== false);
/** Same rule for the remote and "Send to". */
export const remoteControlEnabled = derived(serverPresentation, value => value.remoteControlEnabled !== false);
export async function loadServerPresentation() {
  try {
    const response = await fetch('/api/v1/server', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (response.ok) serverPresentation.set(await response.json());
  } catch { /* Optional presentation never blocks authentication or playback. */ }
}

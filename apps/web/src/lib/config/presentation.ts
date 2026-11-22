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
  clusterEnabled?: boolean;
  remoteControlEnabled?: boolean;
  homeLibraryCardsEnabled?: boolean;
}>({ serverName: '', welcomeMessage: '', watchTogetherEnabled: false, clusterEnabled: false, homeLibraryCardsEnabled: false });
/** Advanced modules (SyncPlay, Cluster/Nodes) are disabled by default for out-of-the-box simplicity. */
export const watchTogetherEnabled = derived(serverPresentation, value => value.watchTogetherEnabled === true);
export const clusterEnabled = derived(serverPresentation, value => value.clusterEnabled === true);
/** Same rule for the remote and "Send to". */
export const remoteControlEnabled = derived(serverPresentation, value => value.remoteControlEnabled !== false);
/** Visual library cards row on the home page: disabled by default. */
export const homeLibraryCardsEnabled = derived(serverPresentation, value => value.homeLibraryCardsEnabled === true);
export async function loadServerPresentation() {
  try {
    const response = await fetch('/api/v1/server', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (response.ok) serverPresentation.set(await response.json());
  } catch { /* Optional presentation never blocks authentication or playback. */ }
}

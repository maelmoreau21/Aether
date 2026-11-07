/**
 * Starting or joining a "Watch together" session from anywhere in the interface.
 *
 * The catalogue page owns the player, so it listens for `start-watch-party` and cancels the
 * event once it has taken it. When no catalogue is mounted (settings, people, statistics…),
 * nobody cancels it and the interface goes to the home page with `?party=`, where the
 * catalogue joins the session on load. Invitation links use the same `?party=` address.
 */
import { goto } from '$app/navigation';
import { resolve } from '$app/paths';

export const START_WATCH_PARTY = 'start-watch-party';
export interface StartWatchPartyDetail { partyId: string; mediaId?: string; workId?: string }

/** Query string of the home page that joins a session on load. */
export function partyQuery(partyId: string, mediaId?: string): string {
  const params = new URLSearchParams({ party: partyId });
  if (mediaId) params.set('media', mediaId);
  return `?${params}`;
}

export function startWatchParty(detail: StartWatchPartyDetail): void {
  const taken = !window.dispatchEvent(new CustomEvent<StartWatchPartyDetail>(START_WATCH_PARTY, { detail, cancelable: true }));
  if (taken) return;
  // eslint-disable-next-line svelte/no-navigation-without-resolve
  void goto(`${resolve('/')}${partyQuery(detail.partyId, detail.mediaId || detail.workId)}`);
}

/** Session id and media requested by the current address, if any. */
export function partyFromLocation(search: string): { partyId: string; mediaId?: string } | null {
  const params = new URLSearchParams(search);
  const partyId = params.get('party')?.trim();
  if (!partyId || partyId.length > 128) return null;
  const mediaId = params.get('media')?.trim() || undefined;
  return { partyId, mediaId: mediaId && mediaId.length <= 256 ? mediaId : undefined };
}

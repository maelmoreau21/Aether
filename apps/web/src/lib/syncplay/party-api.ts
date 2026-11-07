/** Appels REST des séances partagées utilisés par la fenêtre de création / liste (WatchPartyModal). */
import type { ControlMode, PartySummary, UserInvitePayload } from './types';

import { tr } from '../i18n';
export interface EligibleUser { id: string; login: string; displayName: string }

export interface CatalogWorkOption {
  id: string;
  title: string;
  year?: number | null;
}

export interface CreatePartyRequest {
  name: string;
  mediaId: string;
  controlMode: ControlMode;
  waitForBuffering: boolean;
  inviteUserIds?: string[];
  inviteUsernames?: string[];
}

const SAME_ORIGIN: RequestInit = { credentials: 'same-origin' };

/** Initiales d'affichage (majuscules) d'un nom. */
export function initials(name: string, length = 2): string {
  return name.slice(0, length).toUpperCase();
}

/** Lien d'invitation vers une séance : la page d'accueil de l'application la rejoint (`?party=`). */
export function partyInviteUrl(appUrl: string, partyId: string): string {
  return `${appUrl.endsWith('/') ? appUrl : `${appUrl}/`}?party=${encodeURIComponent(partyId)}`;
}

/** Message d'erreur renvoyé par l'API (`{ error }`), ou `fallback`. */
export async function responseErrorMessage(res: Response, fallback: string): Promise<string> {
  try {
    const body = await res.json();
    if (body.error) return body.error;
  } catch { /* corps absent ou illisible */ }
  return fallback;
}

function postHeaders(csrfToken: string, json = false): Record<string, string> {
  return json
    ? { 'Content-Type': 'application/json', 'x-csrf-token': csrfToken }
    : { 'x-csrf-token': csrfToken };
}

/** Utilisateurs pouvant être invités ; `null` si la recherche échoue (erreurs ignorées). */
export async function searchEligibleUsers(query: string, mediaId?: string): Promise<EligibleUser[] | null> {
  try {
    const cleanQ = query.trim().replace(/^@/, '');
    const mediaParam = mediaId ? `&media_id=${encodeURIComponent(mediaId)}` : '';
    const res = await fetch(`/api/v1/parties/eligible-users?q=${encodeURIComponent(cleanQ)}${mediaParam}`, SAME_ORIGIN);
    if (!res.ok) return null;
    const data = await res.json();
    return data.users ?? [];
  } catch {
    return null;
  }
}

/** Œuvres du catalogue correspondant à la recherche ; `null` en cas d'échec. */
export async function searchCatalogWorks(query: string): Promise<CatalogWorkOption[] | null> {
  try {
    const url = `/api/v1/catalog?${new URLSearchParams({ q: query.trim(), limit: '6' })}`;
    const res = await fetch(url, SAME_ORIGIN);
    if (!res.ok) return null;
    const data = await res.json();
    return (data.items ?? []).map((w: CatalogWorkOption) => ({ id: w.id, title: w.title, year: w.year }));
  } catch {
    return null;
  }
}

export async function fetchActiveParties(): Promise<PartySummary[] | null> {
  try {
    const res = await fetch('/api/v1/parties', SAME_ORIGIN);
    if (!res.ok) return null;
    const data = await res.json();
    return data.parties ?? [];
  } catch {
    return null;
  }
}

export async function fetchPartyInvites(): Promise<UserInvitePayload[] | null> {
  try {
    const res = await fetch('/api/v1/parties/invites', SAME_ORIGIN);
    if (!res.ok) return null;
    const data = await res.json();
    return data.invites ?? [];
  } catch {
    return null;
  }
}

/** Crée une séance ; lève une `Error` au message affichable en cas d'échec. */
export async function createParty(request: CreatePartyRequest, csrfToken: string): Promise<{ id: string }> {
  const res = await fetch('/api/v1/parties', {
    method: 'POST',
    ...SAME_ORIGIN,
    headers: postHeaders(csrfToken, true),
    body: JSON.stringify(request)
  });
  if (!res.ok) throw new Error(await responseErrorMessage(res, tr('unable_to_create_the_session')));
  return res.json();
}

/** Accepte une invitation ; lève une `Error` au message affichable en cas d'échec. */
export async function acceptPartyInvite(token: string, csrfToken: string): Promise<void> {
  const res = await fetch(`/api/v1/parties/invites/${encodeURIComponent(token)}/accept`, {
    method: 'POST',
    ...SAME_ORIGIN,
    headers: postHeaders(csrfToken)
  });
  if (!res.ok) throw new Error(await responseErrorMessage(res, tr('the_invitation_has_expired_or_is')));
}

/** Rejoint une séance ; lève une `Error` au message affichable en cas d'échec. */
export async function joinParty(partyId: string, csrfToken: string): Promise<void> {
  const res = await fetch(`/api/v1/parties/${encodeURIComponent(partyId)}/join`, {
    method: 'POST',
    ...SAME_ORIGIN,
    headers: postHeaders(csrfToken)
  });
  if (!res.ok) throw new Error(await responseErrorMessage(res, tr('unable_to_join_this_session')));
}

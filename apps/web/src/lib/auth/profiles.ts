import { tr } from '../i18n';

export interface AccountProfile { id: string; name: string; hasPin: boolean; restricted: boolean; isOwner: boolean; isCurrent: boolean }
export interface AccountProfiles { ownerId: string; currentId: string; canManage: boolean; maxProfiles: number; profiles: AccountProfile[] }

const BASE = '/api/v1/account/profiles';
/** Set by the login page so "Who's watching?" skips itself for single-profile accounts. */
export const PROFILES_AFTER_LOGIN = 'aether-profiles-after-login';
export const KID_MAXIMUM_AGE = 12;

async function send(method: string, path: string, csrf: string, body?: unknown): Promise<Response> {
  return fetch(path, {
    method, credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json', 'x-csrf-token': csrf },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000)
  });
}

async function failure(response: Response): Promise<Error> {
  const code = ((await response.json().catch(() => ({}))) as { error?: string }).error;
  if (response.status === 429) return new Error(tr('too_many_attempts_wait_before_trying'));
  if (code === 'invalid_pin') return new Error(tr('profiles_wrong_pin'));
  if (code === 'profile_limit') return new Error(tr('profiles_limit_reached'));
  if (code === 'invalid_name') return new Error(tr('profiles_invalid_name'));
  return new Error(tr('account_profiles_unavailable'));
}

export async function loadProfiles(): Promise<AccountProfiles> {
  const response = await fetch(BASE, { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (!response.ok) throw await failure(response);
  return response.json();
}

export async function createProfile(csrf: string, name: string, kid: boolean): Promise<void> {
  const response = await send('POST', BASE, csrf, kid ? { name, maximumAge: KID_MAXIMUM_AGE } : { name });
  if (!response.ok) throw await failure(response);
}

export async function deleteProfile(csrf: string, id: string): Promise<void> {
  const response = await send('DELETE', `${BASE}/${encodeURIComponent(id)}`, csrf);
  if (!response.ok) throw await failure(response);
}

/** Sets or (with `null`) removes a PIN; a profile changing its own PIN passes its current one. */
export async function setProfilePin(csrf: string, id: string, pin: string | null, currentPin?: string): Promise<void> {
  const response = await send('PUT', `${BASE}/${encodeURIComponent(id)}/pin`, csrf, currentPin ? { pin, currentPin } : { pin });
  if (!response.ok) throw await failure(response);
}

/** Opens a session for the chosen profile; the server closes the current one. */
export async function switchProfile(csrf: string, id: string, pin?: string): Promise<void> {
  const response = await send('POST', `${BASE}/${encodeURIComponent(id)}/switch`, csrf, pin ? { pin } : {});
  if (!response.ok) throw await failure(response);
}

export const isValidPin = (pin: string): boolean => /^\d{4,6}$/.test(pin);
export const profileInitial = (name: string): string => (name.trim().charAt(0) || '?').toLocaleUpperCase();

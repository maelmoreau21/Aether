import { tr } from '../i18n';

/** An account signed in on this browser: the active one, or one set aside. */
export interface DeviceAccount { id: string; name: string; login: string; current: boolean; hasPin: boolean }
export interface DeviceAccounts { accounts: DeviceAccount[]; max: number }

interface Listed { accounts: { user: { id: string; login: string; displayName?: string }; current: boolean; hasPin?: boolean }[]; max: number }

/** Several accounts on this device: the app asks who is watching when it opens. */
const SHARED_DEVICE = 'aether-shared-device';
/** Set once an account was chosen in this app session (tab, TV or desktop launch). */
const CHOSEN = 'aether-account-chosen';

/** Raised when the chosen account asks for its short PIN. */
export class PinRequired extends Error {}

function remember(storage: () => Storage, key: string, on: boolean): void {
  try { if (on) storage().setItem(key, '1'); else storage().removeItem(key); } catch { /* private mode */ }
}
function recalled(storage: () => Storage, key: string): boolean {
  try { return storage().getItem(key) === '1'; } catch { return false; }
}
export const markAccountChosen = (): void => remember(() => sessionStorage, CHOSEN, true);
/** Whether the launch picker should open: several accounts known here and none chosen yet. */
export const shouldAskWhoIsWatching = (): boolean =>
  recalled(() => localStorage, SHARED_DEVICE) && !recalled(() => sessionStorage, CHOSEN);

const BASE = '/api/v1/auth/accounts';

async function send(path: string, csrf: string, body?: unknown): Promise<void> {
  const response = await fetch(`${BASE}/${path}`, {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json', 'x-csrf-token': csrf },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000)
  });
  if (response.ok) return;
  const code = ((await response.json().catch(() => ({}))) as { error?: string }).error;
  if (code === 'account_limit') throw new Error(tr('accounts_limit_reached'));
  if (code === 'account_not_found') throw new Error(tr('accounts_session_ended'));
  if (code === 'pin_required') throw new PinRequired(tr('accounts_pin_required'));
  if (code === 'invalid_pin') throw new Error(tr('accounts_pin_invalid'));
  if (code === 'try_later' || response.status === 429) throw new Error(tr('accounts_pin_wait'));
  throw new Error(tr('accounts_unavailable'));
}

/** Works signed out too: the sign-in page offers the accounts set aside. */
export async function loadAccounts(signal?: AbortSignal): Promise<DeviceAccounts> {
  const response = await fetch(BASE, { credentials: 'same-origin', cache: 'no-store', signal: signal ?? AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error(tr('accounts_unavailable'));
  const data = (await response.json()) as Listed;
  remember(() => localStorage, SHARED_DEVICE, data.accounts.length > 1);
  return {
    max: data.max,
    accounts: data.accounts.map(({ user, current, hasPin }) => ({ id: user.id, login: user.login, name: user.displayName || user.login, current, hasPin: hasPin === true }))
  };
}

/** Makes an account set aside the active one; the caller then reloads the application. */
export const switchAccount = (csrf: string, id: string, pin?: string): Promise<void> =>
  send('switch', csrf, pin ? { userId: id, pin } : { userId: id });

/** Sets (4 to 6 digits) or removes (null) the short PIN asked when switching to the current account. */
export async function setAccountPin(csrf: string, userId: string, pin: string | null): Promise<void> {
  const response = await fetch(`/api/v1/account/profiles/${encodeURIComponent(userId)}/pin`, {
    method: 'PUT', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json', 'x-csrf-token': csrf },
    body: JSON.stringify({ pin }), signal: AbortSignal.timeout(15000)
  });
  if (response.ok) return;
  const code = ((await response.json().catch(() => ({}))) as { error?: string }).error;
  throw new Error(code === 'invalid_pin_format' ? tr('accounts_pin_format') : tr('accounts_unavailable'));
}
/** Sets the active account aside (it stays signed in) so another one can sign in. */
export const parkAccount = (csrf: string): Promise<void> => send('park', csrf);
/** Signs an account set aside out of this browser. */
export const forgetAccount = (csrf: string, id: string): Promise<void> => send('forget', csrf, { userId: id });

export const accountInitial = (name: string): string => (name.trim().charAt(0) || '?').toLocaleUpperCase();

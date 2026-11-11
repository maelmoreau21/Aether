import { tr } from '../i18n';

/** An account signed in on this browser: the active one, or one set aside. */
export interface DeviceAccount { id: string; name: string; login: string; current: boolean }
export interface DeviceAccounts { accounts: DeviceAccount[]; max: number }

interface Listed { accounts: { user: { id: string; login: string; displayName?: string }; current: boolean }[]; max: number }

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
  throw new Error(tr('accounts_unavailable'));
}

/** Works signed out too: the sign-in page offers the accounts set aside. */
export async function loadAccounts(signal?: AbortSignal): Promise<DeviceAccounts> {
  const response = await fetch(BASE, { credentials: 'same-origin', cache: 'no-store', signal: signal ?? AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error(tr('accounts_unavailable'));
  const data = (await response.json()) as Listed;
  return {
    max: data.max,
    accounts: data.accounts.map(({ user, current }) => ({ id: user.id, login: user.login, name: user.displayName || user.login, current }))
  };
}

/** Makes an account set aside the active one; the caller then reloads the application. */
export const switchAccount = (csrf: string, id: string): Promise<void> => send('switch', csrf, { userId: id });
/** Sets the active account aside (it stays signed in) so another one can sign in. */
export const parkAccount = (csrf: string): Promise<void> => send('park', csrf);
/** Signs an account set aside out of this browser. */
export const forgetAccount = (csrf: string, id: string): Promise<void> => send('forget', csrf, { userId: id });

export const accountInitial = (name: string): string => (name.trim().charAt(0) || '?').toLocaleUpperCase();

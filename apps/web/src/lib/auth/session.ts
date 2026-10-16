import { writable } from 'svelte/store';

export interface AuthUser { id: string; login: string; role: 'administrator' | 'user' }
export interface Session { user: AuthUser; csrfToken: string }
// Memory only. The session credential is an HttpOnly cookie managed by Core.
export const session = writable<Session | null>(null);

export async function readSession(): Promise<Session | null> {
  const response = await fetch('/api/v1/auth/me', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (response.status === 401) { session.set(null); return null; }
  if (!response.ok) throw new Error('Impossible de vérifier la session. Réessayez.');
  const current: Session = await response.json();
  session.set(current);
  return current;
}

export async function login(login: string, password: string): Promise<void> {
  const response = await fetch('/api/v1/auth/login', {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json' }, body: JSON.stringify({ login, password }),
    signal: AbortSignal.timeout(15000)
  });
  if (response.status === 429) throw new Error('Trop de tentatives. Patientez avant de réessayer.');
  if (response.status === 401 || response.status === 400) throw new Error('Identifiants incorrects.');
  if (!response.ok) throw new Error('La connexion est indisponible. Réessayez.');
  session.set(await response.json() as Session);
}

export async function logout(current: Session): Promise<void> {
  const response = await fetch('/api/v1/auth/logout', {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'x-csrf-token': current.csrfToken }, signal: AbortSignal.timeout(10000)
  });
  if (!response.ok && response.status !== 401) throw new Error('Déconnexion impossible. Réessayez.');
  session.set(null);
}

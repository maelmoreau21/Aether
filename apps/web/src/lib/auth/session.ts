import { writable } from 'svelte/store';

export interface AuthUser { id: string; login: string; role: 'administrator' | 'user' }
export interface Session { user: AuthUser; csrfToken: string }

type SessionGenerationListener = (generation: number, current: Session | null) => void;

let currentSession: Session | null = null;
let sessionGeneration = 0;
const generationListeners = new Set<SessionGenerationListener>();
const baseSession = writable<Session | null>(null);

function isSameSession(a: Session | null, b: Session | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.user.id === b.user.id &&
    a.user.login === b.user.login &&
    a.user.role === b.user.role &&
    a.csrfToken === b.csrfToken
  );
}

function applySession(next: Session | null, forceNewGeneration = false): number {
  const changed = !isSameSession(currentSession, next);
  currentSession = next;
  if (changed || forceNewGeneration) {
    sessionGeneration += 1;
    for (const listener of generationListeners) {
      listener(sessionGeneration, currentSession);
    }
  }
  baseSession.set(currentSession);
  return sessionGeneration;
}

// Memory only. The session credential is an HttpOnly cookie managed by Core.
export const session = {
  subscribe: baseSession.subscribe,
  set(next: Session | null): void {
    applySession(next, false);
  },
  update(updater: (value: Session | null) => Session | null): void {
    applySession(updater(currentSession), false);
  }
};

export function getSessionGeneration(): number {
  return sessionGeneration;
}

export function onSessionGenerationChange(listener: SessionGenerationListener): () => void {
  generationListeners.add(listener);
  return () => {
    generationListeners.delete(listener);
  };
}

export function invalidateSession(expectedGeneration?: number): boolean {
  if (expectedGeneration !== undefined && expectedGeneration !== sessionGeneration) {
    return false;
  }
  applySession(null, true);
  return true;
}

export async function readSession(): Promise<Session | null> {
  const requestGeneration = sessionGeneration;
  const response = await fetch('/api/v1/auth/me', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (response.status === 401) {
    if (sessionGeneration === requestGeneration) {
      applySession(null, true);
    }
    return currentSession;
  }
  if (!response.ok) throw new Error('Impossible de vérifier la session. Réessayez.');
  const current: Session = await response.json();
  if (sessionGeneration !== requestGeneration) {
    return currentSession;
  }
  applySession(current, false);
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
  applySession(await response.json() as Session, true);
}

export async function logout(current: Session): Promise<void> {
  const response = await fetch('/api/v1/auth/logout', {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'x-csrf-token': current.csrfToken }, signal: AbortSignal.timeout(10000)
  });
  if (!response.ok && response.status !== 401) throw new Error('Déconnexion impossible. Réessayez.');
  applySession(null, true);
}

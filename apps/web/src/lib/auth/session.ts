import { writable } from 'svelte/store';

export interface AuthUser { id: string; login: string; role: 'administrator' | 'user'; displayName?: string | null; sso?: boolean | null; disabledAt?: number | null; }
export interface Session { user: AuthUser; csrfToken: string }

type SessionGenerationListener = (generation: number, current: Session | null) => void;

let currentSession: Session | null = null;
let sessionGeneration = 0;
let authActionSeq = 0;
let manualOverrideSeq = 0;
let authMutationQueue: Promise<void> = Promise.resolve();
let pendingAuthMutations = 0;
const generationListeners = new Set<SessionGenerationListener>();
const baseSession = writable<Session | null>(null);

function enqueueAuthMutation<T>(operation: () => Promise<T>): Promise<T> {
  authActionSeq += 1;
  pendingAuthMutations += 1;
  const run = authMutationQueue.then(operation, operation);
  authMutationQueue = run.then(
    () => {
      pendingAuthMutations -= 1;
    },
    () => {
      pendingAuthMutations -= 1;
    }
  );
  return run;
}

function isSameSession(a: Session | null, b: Session | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.user.id === b.user.id &&
    a.user.login === b.user.login &&
    a.user.role === b.user.role &&
    a.user.displayName === b.user.displayName &&
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
    authActionSeq += 1;
    manualOverrideSeq += 1;
    applySession(next, false);
  },
  update(updater: (value: Session | null) => Session | null): void {
    authActionSeq += 1;
    manualOverrideSeq += 1;
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
  if (pendingAuthMutations > 0) {
    await authMutationQueue;
  }
  const requestGeneration = sessionGeneration;
  const requestActionSeq = authActionSeq;
  const response = await fetch('/api/v1/auth/me', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (response.status === 401) {
    if (sessionGeneration === requestGeneration && authActionSeq === requestActionSeq) {
      applySession(null, true);
    }
    return currentSession;
  }
  if (!response.ok) {
    if (sessionGeneration !== requestGeneration || authActionSeq !== requestActionSeq) {
      return currentSession;
    }
    throw new Error('Impossible de vérifier la session. Réessayez.');
  }
  const current: Session = await response.json();
  if (sessionGeneration !== requestGeneration || authActionSeq !== requestActionSeq) {
    return currentSession;
  }
  applySession(current, false);
  return current;
}

export async function bootstrapAdmin(login: string, password: string, bootstrapToken?: string): Promise<void> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    const response = await fetch('/api/v1/auth/bootstrap', {
      method: 'POST',
      credentials: 'same-origin',
      cache: 'no-store',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ login, password, bootstrap_token: bootstrapToken }),
      signal: AbortSignal.timeout(15000)
    });
    if (response.status === 403) {
      const err = await response.json().catch(() => null);
      if (err?.error === 'http_bootstrap_disabled') {
        throw new Error("L'initialisation via le Web est désactivée. Utilisez la commande locale 'media-bootstrap'.");
      }
      throw new Error("L'initialisation du serveur est déjà clôturée.");
    }
    if (response.status === 401) {
      const err = await response.json().catch(() => null);
      if (err?.error === 'invalid_bootstrap_token') {
        throw new Error("Code d'initialisation invalide. Consultez la console du serveur ou le fichier .bootstrap_token.");
      }
      throw new Error("Autorisation d'initialisation refusée.");
    }
    if (response.status === 429) throw new Error('Trop de requêtes. Patientez avant de réessayer.');
    if (response.status === 400) {
      const err = await response.json().catch(() => null);
      if (err?.error === 'invalid_password') {
        throw new Error('Le mot de passe doit comporter au moins 15 caractères.');
      }
      if (err?.error === 'invalid_login') {
        throw new Error("L'identifiant est invalide (minuscules, chiffres, points, tirets uniquement).");
      }
      throw new Error('Identifiant ou mot de passe invalide.');
    }
    if (!response.ok) throw new Error("L'initialisation a échoué. Réessayez.");
    const authenticated = (await response.json()) as Session;
    if (overrideSeq !== manualOverrideSeq) {
      throw new Error('La session a changé pendant l’initialisation.');
    }
    applySession(authenticated, true);
  });
}

export async function login(login: string, password: string): Promise<void> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    const response = await fetch('/api/v1/auth/login', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: { 'content-type': 'application/json' }, body: JSON.stringify({ login, password }),
      signal: AbortSignal.timeout(15000)
    });
    if (response.status === 429) throw new Error('Trop de tentatives. Patientez avant de réessayer.');
    if (response.status === 401 || response.status === 400) throw new Error('Identifiants incorrects.');
    if (!response.ok) throw new Error('La connexion est indisponible. Réessayez.');
    const authenticated = (await response.json()) as Session;
    if (overrideSeq !== manualOverrideSeq) {
      throw new Error('La session a changé pendant la connexion.');
    }
    applySession(authenticated, true);
  });
}

export async function logout(current: Session): Promise<void> {
  return enqueueAuthMutation(async () => {
    if (currentSession !== null && !isSameSession(currentSession, current)) {
      return;
    }
    const requestGeneration = sessionGeneration;
    const overrideSeq = manualOverrideSeq;
    const response = await fetch('/api/v1/auth/logout', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: { 'x-csrf-token': current.csrfToken }, signal: AbortSignal.timeout(10000)
    });
    if (!response.ok && response.status !== 401) throw new Error('Déconnexion impossible. Réessayez.');
    if (overrideSeq === manualOverrideSeq && (sessionGeneration === requestGeneration || isSameSession(currentSession, current))) {
      applySession(null, true);
    }
  });
}

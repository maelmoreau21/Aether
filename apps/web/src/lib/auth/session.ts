import { writable } from 'svelte/store';
import { deviceRequest } from './quick-connect';
import { getAssertion, type AssertionJSON, type RequestOptionsJSON } from './webauthn';

import { tr } from '../i18n';
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
    throw new Error(tr('unable_to_verify_the_session_try'));
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
        throw new Error(tr('web_initialisation_is_disabled_use_the'));
      }
      throw new Error(tr('server_initialisation_is_already_closed'));
    }
    if (response.status === 401) {
      const err = await response.json().catch(() => null);
      if (err?.error === 'invalid_bootstrap_token') {
        throw new Error(tr('invalid_initialisation_code_check_the_server'));
      }
      throw new Error(tr('initialisation_authorisation_refused'));
    }
    if (response.status === 429) throw new Error(tr('too_many_requests_wait_before_trying'));
    if (response.status === 400) {
      const err = await response.json().catch(() => null);
      if (err?.error === 'invalid_password') {
        throw new Error(tr('the_password_must_be_at_least'));
      }
      if (err?.error === 'invalid_login') {
        throw new Error(tr('the_username_is_invalid_lowercase_letters'));
      }
      throw new Error(tr('invalid_username_or_password'));
    }
    if (!response.ok) throw new Error(tr('initialisation_failed_try_again'));
    const authenticated = (await response.json()) as Session;
    if (overrideSeq !== manualOverrideSeq) {
      throw new Error(tr('the_session_changed_during_initialisation'));
    }
    applySession(authenticated, true);
  });
}

/** Password accepted; the account asks for a second factor before Core opens a session. */
export interface MfaChallenge { mfaToken: string; methods: ('totp' | 'recovery' | 'passkey')[]; passkey?: RequestOptionsJSON }
export type SecondFactor = { code: string } | { recoveryCode: string } | { passkey: AssertionJSON };

async function postSignIn(path: string, body: unknown, failures: Record<number, string>): Promise<Session | MfaChallenge> {
  const response = await fetch(path, {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json' }, body: JSON.stringify(body),
    signal: AbortSignal.timeout(15000)
  });
  if (response.status === 429) throw new Error(tr('too_many_attempts_wait_before_trying'));
  const failure = failures[response.status];
  if (failure) throw new Error(failure);
  if (!response.ok) throw new Error(tr('sign_in_is_unavailable_try_again'));
  return response.json();
}

function finishSignIn(result: Session | MfaChallenge, overrideSeq: number): MfaChallenge | null {
  if (overrideSeq !== manualOverrideSeq) {
    throw new Error(tr('the_session_changed_during_sign_in'));
  }
  if ('mfaRequired' in result || 'mfaToken' in result) return result as MfaChallenge;
  applySession(result, true);
  return null;
}

/** Resolves to `null` once signed in, or to the second-factor challenge to answer. */
export async function login(login: string, password: string): Promise<MfaChallenge | null> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    const invalid = tr('incorrect_credentials');
    const result = await postSignIn('/api/v1/auth/login', { login, password }, { 400: invalid, 401: invalid });
    return finishSignIn(result, overrideSeq);
  });
}

export async function completeSecondFactor(challenge: MfaChallenge, factor: SecondFactor): Promise<void> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    const response = await fetch('/api/v1/auth/login/mfa', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: { 'content-type': 'application/json' }, body: JSON.stringify({ mfaToken: challenge.mfaToken, ...factor }),
      signal: AbortSignal.timeout(15000)
    });
    if (response.status === 429) throw new Error(tr('too_many_attempts_wait_before_trying'));
    if (response.status === 401) {
      const code = (await response.json().catch(() => null))?.error;
      throw new Error(code === 'mfa_expired' ? tr('mfa_step_expired_sign_in_again') : tr('mfa_code_incorrect'));
    }
    if (!response.ok) throw new Error(tr('sign_in_is_unavailable_try_again'));
    finishSignIn(await response.json(), overrideSeq);
  });
}

export async function loginWithPasskey(): Promise<void> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    const optionsResponse = await fetch('/api/v1/auth/passkey/options', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000)
    });
    if (optionsResponse.status === 429) throw new Error(tr('too_many_attempts_wait_before_trying'));
    if (!optionsResponse.ok) throw new Error(tr('sign_in_is_unavailable_try_again'));
    const assertion = await getAssertion(await optionsResponse.json());
    const result = await postSignIn('/api/v1/auth/passkey/login', assertion, { 400: tr('passkey_not_recognised'), 401: tr('passkey_not_recognised') });
    finishSignIn(result, overrideSeq);
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
    if (!response.ok && response.status !== 401) throw new Error(tr('sign_out_failed_try_again'));
    if (overrideSeq === manualOverrideSeq && (sessionGeneration === requestGeneration || isSameSession(currentSession, current))) {
      applySession(null, true);
    }
  });
}

export async function pollDeviceConnection(secret: string, signal: AbortSignal): Promise<boolean> {
  return enqueueAuthMutation(async () => {
    const overrideSeq = manualOverrideSeq;
    if (signal.aborted || currentSession) return false;
    const response = await deviceRequest<Session & { status: 'pending' | 'authenticated' }>('poll', { secret }, undefined, signal);
    if (signal.aborted || overrideSeq !== manualOverrideSeq) return false;
    if (response.status !== 'authenticated') return false;
    applySession({ user: response.user, csrfToken: response.csrfToken }, true);
    return true;
  });
}

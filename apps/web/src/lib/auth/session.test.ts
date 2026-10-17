import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  getSessionGeneration,
  invalidateSession,
  login,
  logout,
  onSessionGenerationChange,
  readSession,
  session,
  type Session
} from './session';

const current: Session = { user: { id: 'account-id', login: 'operator', role: 'administrator' }, csrfToken: 'csrf-test-value' };
const secondUser: Session = { user: { id: 'user-b-id', login: 'viewer', role: 'user' }, csrfToken: 'csrf-user-b' };

afterEach(() => {
  vi.unstubAllGlobals();
  session.set(null);
});

describe('cookie based authentication client', () => {
  it('recovers identity from Core and clears it when the session expires', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(current)).mockResolvedValueOnce(new Response(null, { status: 401 })));
    expect(await readSession()).toEqual(current);
    expect(get(session)).toEqual(current);
    expect(await readSession()).toBeNull();
    expect(get(session)).toBeNull();
  });

  it('uses the same invalid-credentials message and leaves no authenticated state', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 401 })));
    await expect(login('unknown', 'wrong')).rejects.toThrow('Identifiants incorrects.');
    expect(get(session)).toBeNull();
  });

  it('reports throttling without claiming login succeeded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 429 })));
    await expect(login('operator', 'wrong')).rejects.toThrow('Trop de tentatives.');
    expect(get(session)).toBeNull();
  });

  it('logs out with the CSRF header and clears memory only after Core accepts', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(null, { status: 403 })).mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    session.set(current);
    await expect(logout(current)).rejects.toThrow('Déconnexion impossible.');
    expect(get(session)).toEqual(current);
    await logout(current);
    expect(get(session)).toBeNull();
    expect(fetchMock).toHaveBeenLastCalledWith('/api/v1/auth/logout', expect.objectContaining({ method: 'POST', credentials: 'same-origin', headers: { 'x-csrf-token': 'csrf-test-value' } }));
  });

  it('increments session generation on login, logout, account switch, and expiration', async () => {
    const generations: number[] = [];
    const unsubscribe = onSessionGenerationChange(gen => generations.push(gen));
    try {
      const initialGen = getSessionGeneration();
      vi.stubGlobal(
        'fetch',
        vi
          .fn()
          .mockResolvedValueOnce(Response.json(current))
          .mockResolvedValueOnce(new Response(null, { status: 204 }))
          .mockResolvedValueOnce(Response.json(secondUser))
      );

      await login('operator', 'secret');
      const genAfterLoginA = getSessionGeneration();
      expect(genAfterLoginA).toBeGreaterThan(initialGen);

      await logout(current);
      const genAfterLogoutA = getSessionGeneration();
      expect(genAfterLogoutA).toBeGreaterThan(genAfterLoginA);

      await login('viewer', 'secret2');
      const genAfterLoginB = getSessionGeneration();
      expect(genAfterLoginB).toBeGreaterThan(genAfterLogoutA);

      expect(generations).toEqual([genAfterLoginA, genAfterLogoutA, genAfterLoginB]);
    } finally {
      unsubscribe();
    }
  });

  it('ignores a stale 401 from readSession if a new login completed while it was in flight', async () => {
    let resolveMe!: (value: Response) => void;
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/auth/me') {
        return new Promise<Response>(resolve => {
          resolveMe = resolve;
        });
      }
      if (url === '/api/v1/auth/login') {
        return Promise.resolve(Response.json(secondUser));
      }
      return Promise.reject(new Error('unexpected url'));
    });
    vi.stubGlobal('fetch', fetchMock);

    const staleMePromise = readSession();
    await login('viewer', 'secret2');
    const genB = getSessionGeneration();
    expect(get(session)).toEqual(secondUser);

    resolveMe(new Response(null, { status: 401 }));
    await staleMePromise;

    expect(get(session)).toEqual(secondUser);
    expect(getSessionGeneration()).toBe(genB);
    expect(invalidateSession(genB - 1)).toBe(false);
    expect(get(session)).toEqual(secondUser);
  });

  it('prevents a slow logout from session A from wiping out a newer login of session B', async () => {
    let resolveLogoutA!: (value: Response) => void;
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/auth/logout') {
        return new Promise<Response>(resolve => {
          resolveLogoutA = resolve;
        });
      }
      if (url === '/api/v1/auth/login') {
        return Promise.resolve(Response.json(secondUser));
      }
      return Promise.reject(new Error('unexpected url'));
    });
    vi.stubGlobal('fetch', fetchMock);

    session.set(current);
    const logoutPromise = logout(current);

    // User B logs in before A's logout response comes back
    await login('viewer', 'secret2');
    const genB = getSessionGeneration();
    expect(get(session)).toEqual(secondUser);

    // Now A's logout finishes
    resolveLogoutA(new Response(null, { status: 204 }));
    await logoutPromise;

    // Session B must remain active and generation must not be overwritten
    expect(get(session)).toEqual(secondUser);
    expect(getSessionGeneration()).toBe(genB);
  });
});

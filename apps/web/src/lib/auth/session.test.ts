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

  it('sequences login of session B strictly after a slow logout of session A so logout Set-Cookie cannot wipe B cookie', async () => {
    let resolveLogoutA!: (value: Response) => void;
    const events: string[] = [];
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/auth/logout') {
        events.push('logout-A-request');
        return new Promise<Response>(resolve => {
          resolveLogoutA = (res: Response) => {
            events.push('logout-A-response');
            resolve(res);
          };
        });
      }
      if (url === '/api/v1/auth/login') {
        events.push('login-B-request');
        return Promise.resolve().then(() => {
          events.push('login-B-response');
          return Response.json(secondUser);
        });
      }
      return Promise.reject(new Error('unexpected url'));
    });
    vi.stubGlobal('fetch', fetchMock);

    session.set(current);
    const logoutPromise = logout(current);
    const loginPromise = login('viewer', 'secret2');

    // Flush microtasks: logout request is in flight, login B must NOT have sent its HTTP request yet
    await Promise.resolve();
    await Promise.resolve();
    expect(events).toEqual(['logout-A-request']);

    // Now A's delayed logout response arrives -> login B is dispatched only after logout A completes
    resolveLogoutA(new Response(null, { status: 204 }));
    await Promise.all([logoutPromise, loginPromise]);

    expect(events).toEqual([
      'logout-A-request',
      'logout-A-response',
      'login-B-request',
      'login-B-response'
    ]);
    expect(get(session)).toEqual(secondUser);
  });

  it('sequences two concurrent logins so a slow first login response cannot arrive after and overwrite the second login cookie', async () => {
    let resolveLoginA!: (value: Response) => void;
    const events: string[] = [];
    const fetchMock = vi.fn().mockImplementation((url: string, init?: RequestInit) => {
      if (url === '/api/v1/auth/login') {
        const body = JSON.parse(String(init?.body ?? '{}')) as { login?: string };
        if (body.login === 'operator') {
          events.push('login-A-request');
          return new Promise<Response>(resolve => {
            resolveLoginA = (res: Response) => {
              events.push('login-A-response');
              resolve(res);
            };
          });
        }
        if (body.login === 'viewer') {
          events.push('login-B-request');
          return Promise.resolve().then(() => {
            events.push('login-B-response');
            return Response.json(secondUser);
          });
        }
      }
      return Promise.reject(new Error('unexpected url'));
    });
    vi.stubGlobal('fetch', fetchMock);

    const loginAPromise = login('operator', 'secret1');
    const loginBPromise = login('viewer', 'secret2');

    // Flush microtasks: login B must wait for login A's HTTP response before sending its own request
    await Promise.resolve();
    await Promise.resolve();
    expect(events).toEqual(['login-A-request']);

    resolveLoginA(Response.json(current));
    await Promise.all([loginAPromise, loginBPromise]);

    expect(events).toEqual([
      'login-A-request',
      'login-A-response',
      'login-B-request',
      'login-B-response'
    ]);
    expect(get(session)).toEqual(secondUser);
  });

  it('waits for an in-flight auth mutation before readSession queries /api/v1/auth/me', async () => {
    let resolveLogoutA!: (value: Response) => void;
    const events: string[] = [];
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === '/api/v1/auth/logout') {
        events.push('logout-request');
        return new Promise<Response>(resolve => {
          resolveLogoutA = (res: Response) => {
            events.push('logout-response');
            resolve(res);
          };
        });
      }
      if (url === '/api/v1/auth/me') {
        events.push('me-request');
        return Promise.resolve(new Response(null, { status: 401 }));
      }
      return Promise.reject(new Error('unexpected url'));
    });
    vi.stubGlobal('fetch', fetchMock);

    session.set(current);
    const logoutPromise = logout(current);
    const mePromise = readSession();

    await Promise.resolve();
    await Promise.resolve();
    expect(events).toEqual(['logout-request']);

    resolveLogoutA(new Response(null, { status: 204 }));
    const result = await mePromise;
    await logoutPromise;

    expect(events).toEqual(['logout-request', 'logout-response', 'me-request']);
    expect(result).toBeNull();
    expect(get(session)).toBeNull();
  });
});

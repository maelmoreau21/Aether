import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { login, logout, readSession, session, type Session } from './session';

const current: Session = { user: { id:'account-id', login:'operator', role:'administrator' }, csrfToken:'csrf-test-value' };
afterEach(() => { vi.unstubAllGlobals(); session.set(null); });
describe('cookie based authentication client', () => {
  it('recovers identity from Core and clears it when the session expires', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(current)).mockResolvedValueOnce(new Response(null, { status:401 })));
    expect(await readSession()).toEqual(current);
    expect(get(session)).toEqual(current);
    expect(await readSession()).toBeNull();
    expect(get(session)).toBeNull();
  });
  it('uses the same invalid-credentials message and leaves no authenticated state', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status:401 })));
    await expect(login('unknown', 'wrong')).rejects.toThrow('Identifiants incorrects.');
    expect(get(session)).toBeNull();
  });
  it('reports throttling without claiming login succeeded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status:429 })));
    await expect(login('operator', 'wrong')).rejects.toThrow('Trop de tentatives.');
    expect(get(session)).toBeNull();
  });
  it('logs out with the CSRF header and clears memory only after Core accepts', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(null, { status:403 })).mockResolvedValueOnce(new Response(null, { status:204 }));
    vi.stubGlobal('fetch', fetchMock); session.set(current);
    await expect(logout(current)).rejects.toThrow('Déconnexion impossible.');
    expect(get(session)).toEqual(current);
    await logout(current);
    expect(get(session)).toBeNull();
    expect(fetchMock).toHaveBeenLastCalledWith('/api/v1/auth/logout', expect.objectContaining({ method:'POST', credentials:'same-origin', headers:{ 'x-csrf-token':'csrf-test-value' } }));
  });
});

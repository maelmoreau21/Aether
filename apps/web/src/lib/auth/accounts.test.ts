import { afterEach, describe, expect, it, vi } from 'vitest';
import { PinRequired, accountInitial, loadAccounts, markAccountChosen, shouldAskWhoIsWatching, switchAccount } from './accounts';

afterEach(() => vi.unstubAllGlobals());

describe('accounts of this browser', () => {
  it('derives an avatar initial', () => {
    expect(accountInitial('élodie')).toBe('É');
    expect(accountInitial('  ')).toBe('?');
  });

  it('names an account by its display name, else its login', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      max: 6,
      accounts: [
        { user: { id: 'a', login: 'mael', displayName: 'Maël' }, current: true },
        { user: { id: 'b', login: 'invite' }, current: false, hasPin: true }
      ]
    }))));
    expect(await loadAccounts()).toEqual({
      max: 6,
      accounts: [
        { id: 'a', login: 'mael', name: 'Maël', current: true, hasPin: false },
        { id: 'b', login: 'invite', name: 'invite', current: false, hasPin: true }
      ]
    });
  });

  it('asks who is watching once per launch when several accounts share the device', async () => {
    const storage = () => { const data = new Map<string, string>(); return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => { data.set(k, v); }, removeItem: (k: string) => { data.delete(k); }, clear: () => data.clear() }; };
    const session = storage();
    vi.stubGlobal('localStorage', storage());
    vi.stubGlobal('sessionStorage', session);
    const listed = (count: number) => new Response(JSON.stringify({
      max: 6, accounts: Array.from({ length: count }, (_, n) => ({ user: { id: `u${n}`, login: `u${n}` }, current: n === 0 }))
    }));
    vi.stubGlobal('fetch', vi.fn(async () => listed(2)));
    await loadAccounts();
    expect(shouldAskWhoIsWatching()).toBe(true);
    markAccountChosen();
    expect(shouldAskWhoIsWatching()).toBe(false);
    session.clear();
    vi.stubGlobal('fetch', vi.fn(async () => listed(1)));
    await loadAccounts();
    expect(shouldAskWhoIsWatching()).toBe(false);
  });

  it('sends the PIN and tells a missing PIN from a wrong one', async () => {
    const fetched = vi.fn(async () => new Response(JSON.stringify({ error: 'pin_required' }), { status: 403 }));
    vi.stubGlobal('fetch', fetched);
    await expect(switchAccount('', 'b')).rejects.toBeInstanceOf(PinRequired);
    fetched.mockImplementation(async () => new Response(JSON.stringify({ error: 'invalid_pin' }), { status: 403 }));
    const wrong = await switchAccount('', 'b', '1357').catch((cause: unknown) => cause);
    expect(wrong).toBeInstanceOf(Error);
    expect(wrong).not.toBeInstanceOf(PinRequired);
    const [, init] = fetched.mock.calls[1] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({ userId: 'b', pin: '1357' });
  });

  it('asks the server to switch with the CSRF token of the active account', async () => {
    const fetched = vi.fn(async () => new Response(null, { status: 200 }));
    vi.stubGlobal('fetch', fetched);
    await switchAccount('token', 'b');
    const [path, init] = fetched.mock.calls[0] as unknown as [string, RequestInit];
    expect(path).toBe('/api/v1/auth/accounts/switch');
    expect(init.body).toBe('{"userId":"b"}');
    expect((init.headers as Record<string, string>)['x-csrf-token']).toBe('token');
  });
});

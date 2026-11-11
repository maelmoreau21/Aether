import { afterEach, describe, expect, it, vi } from 'vitest';
import { accountInitial, loadAccounts, switchAccount } from './accounts';

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
        { user: { id: 'b', login: 'invite' }, current: false }
      ]
    }))));
    expect(await loadAccounts()).toEqual({
      max: 6,
      accounts: [
        { id: 'a', login: 'mael', name: 'Maël', current: true },
        { id: 'b', login: 'invite', name: 'invite', current: false }
      ]
    });
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

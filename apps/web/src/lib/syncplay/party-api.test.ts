import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  acceptPartyInvite, createParty, fetchActiveParties, initials, joinParty, partyInviteUrl,
  responseErrorMessage, searchCatalogWorks, searchEligibleUsers
} from './party-api';

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

afterEach(() => vi.unstubAllGlobals());

describe('party helpers', () => {
  it('builds initials and invite urls', () => {
    expect(initials('alice')).toBe('AL');
    expect(initials('alice', 1)).toBe('A');
    expect(partyInviteUrl('http://x', 'a b')).toBe('http://x/?party=a%20b');
    expect(partyInviteUrl('http://x/app/', 'p1')).toBe('http://x/app/?party=p1');
  });

  it('extracts API error messages with a fallback', async () => {
    expect(await responseErrorMessage(json({ error: 'nope' }, 400), 'dflt')).toBe('nope');
    expect(await responseErrorMessage(json({}, 400), 'dflt')).toBe('dflt');
    expect(await responseErrorMessage(new Response('not json', { status: 500 }), 'dflt')).toBe('dflt');
  });
});

describe('party api', () => {
  it('strips the leading @ and scopes user search to a media', async () => {
    const fetchMock = vi.fn().mockResolvedValue(json({ users: [{ id: '1', login: 'bob', displayName: 'Bob' }] }));
    vi.stubGlobal('fetch', fetchMock);
    const users = await searchEligibleUsers(' @bob ', 'm 1');
    expect(users).toHaveLength(1);
    expect(fetchMock.mock.calls[0][0]).toBe('/api/v1/parties/eligible-users?q=bob&media_id=m%201');
  });

  it('returns null instead of throwing on failures', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({}, 500)));
    expect(await searchCatalogWorks('x')).toBeNull();
    expect(await fetchActiveParties()).toBeNull();
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')));
    expect(await searchEligibleUsers('x')).toBeNull();
  });

  it('maps catalog results', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ items: [{ id: 'a', title: 'T', year: 2020, extra: 1 }] })));
    expect(await searchCatalogWorks('t')).toEqual([{ id: 'a', title: 'T', year: 2020 }]);
  });

  it('sends the csrf token and surfaces server errors', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(json({ id: 'p1' }))
      .mockResolvedValueOnce(json({ error: 'expiré' }, 410))
      .mockResolvedValueOnce(json({}, 403));
    vi.stubGlobal('fetch', fetchMock);
    const party = await createParty({ name: 'n', mediaId: 'm', controlMode: 'host_only', waitForBuffering: true }, 'tok');
    expect(party.id).toBe('p1');
    expect(fetchMock.mock.calls[0][1].headers['x-csrf-token']).toBe('tok');
    await expect(acceptPartyInvite('t', 'tok')).rejects.toThrow('expiré');
    await expect(joinParty('p', 'tok')).rejects.toThrow('Impossible de rejoindre cette séance.');
  });
});

import { describe, expect, it } from 'vitest';
import { addedAccess, needsApproval } from './extension-access';

describe('addedAccess', () => {
  it('names only the hosts and permissions the new version adds', () => {
    const change = addedAccess(
      { permissions: ['metadata.read', 'images'], httpHosts: ['www.imdb.com'] },
      { permissions: ['metadata.read', 'images'], httpHosts: ['www.imdb.com', 'caching.graphql.imdb.com', 'm.media-amazon.com'] }
    );
    expect(change).toEqual({ permissions: [], httpHosts: ['caching.graphql.imdb.com', 'm.media-amazon.com'] });
    expect(needsApproval(change)).toBe(true);
  });

  it('needs nothing when the new version keeps or narrows its access', () => {
    const change = addedAccess(
      { permissions: ['metadata.read', 'images'], httpHosts: ['a.example', 'b.example'] },
      { permissions: ['metadata.read'], httpHosts: ['a.example'] }
    );
    expect(needsApproval(change)).toBe(false);
  });

  it('treats a missing host list as empty', () => {
    expect(addedAccess({ permissions: [] }, { permissions: ['requests'] })).toEqual({ permissions: ['requests'], httpHosts: [] });
  });
});

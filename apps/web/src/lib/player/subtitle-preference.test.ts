import { describe, expect, it } from 'vitest';
import { preferredSubtitle } from './subtitle-preference';
const tracks = [{ id: 'fr', language: 'fr', label: 'FR', url: '/fr' }, { id: 'forced', language: 'fr', forced: true, label: 'FR forced', url: '/forced' }];
describe('subtitle preferences', () => {
  it('defaults to French forced captions', () => expect(preferredSubtitle(tracks)).toBe('forced'));
  it('preserves an explicit off selection', () => expect(preferredSubtitle(tracks, { enabled: false })).toBeNull());
  it('preserves a manual full-language selection', () => expect(preferredSubtitle(tracks, { enabled: true, language: 'fr', forced: false })).toBe('fr'));
  it('does not substitute an unrelated subtitle for a missing preference', () => expect(preferredSubtitle(tracks, { enabled: true, language: 'ja' })).toBeNull());
});

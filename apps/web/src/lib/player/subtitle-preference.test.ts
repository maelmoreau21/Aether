import { describe, expect, it } from 'vitest';
import { preferredSubtitle } from './subtitle-preference';
const tracks = [{ id: 'fr', language: 'fr', label: 'FR', url: '/fr' }, { id: 'forced', language: 'fr', forced: true, label: 'FR forced', url: '/forced' }];
describe('subtitle preferences', () => {
  it('defaults to French forced captions', () => expect(preferredSubtitle(tracks)).toBe('forced'));
  it('preserves an explicit off selection', () => expect(preferredSubtitle(tracks, { enabled: false })).toBeNull());
  it('preserves a manual full-language selection', () => expect(preferredSubtitle(tracks, { enabled: true, language: 'fr', forced: false })).toBe('fr'));
  it('does not substitute an unrelated subtitle for a missing preference', () => expect(preferredSubtitle(tracks, { enabled: true, language: 'ja' })).toBeNull());
  it('puts SDH first in the chosen language only when the account asks for it', () => {
    const withSdh = [...tracks, { id: 'sdh', language: 'fr', sdh: true, label: 'FR SDH', url: '/sdh' }];
    const full = { enabled: true, language: 'fr', forced: false };
    expect(preferredSubtitle(withSdh, full)).toBe('fr');
    expect(preferredSubtitle(withSdh, full, 'fr', true)).toBe('sdh');
    expect(preferredSubtitle(withSdh, { ...full, forced: true }, 'fr', true)).toBe('forced');
    expect(preferredSubtitle(withSdh, { enabled: false }, 'fr', true)).toBeNull();
  });
});

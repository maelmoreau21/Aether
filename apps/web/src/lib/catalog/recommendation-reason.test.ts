import { describe, expect, it } from 'vitest';
import { recommendationReasonText } from './recommendation-reason';

const t = (key: string, values?: Record<string, string>) => `${key}${values?.title ? `:${values.title}` : ''}`;

describe('recommendationReasonText', () => {
  it('localizes liked and watched seeds', () => {
    expect(recommendationReasonText({ recommendationBecause: { kind: 'liked', title: 'Dune' } }, t)).toBe('recommended_because_liked:Dune');
    expect(recommendationReasonText({ recommendationBecause: { kind: 'watched', title: 'Alien' } }, t)).toBe('recommended_because_watched:Alien');
  });
  it('falls back to discovery or the legacy server text', () => {
    expect(recommendationReasonText({ recommendationBecause: null, recommendationReason: 'À découvrir' }, t)).toBe('recommended_to_discover');
    expect(recommendationReasonText({ recommendationReason: 'Selon votre intérêt pour X' }, t)).toBe('Selon votre intérêt pour X');
  });
});

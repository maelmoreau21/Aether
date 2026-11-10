import { describe, expect, it } from 'vitest';
import { isValidPin, profileInitial } from './profiles';

describe('profiles helpers', () => {
  it('accepts 4 to 6 digit PINs only', () => {
    expect(['1234', '123456'].map(isValidPin)).toEqual([true, true]);
    expect(['123', '1234567', '12a4', ' 1234', ''].map(isValidPin)).toEqual([false, false, false, false, false]);
  });
  it('derives an avatar initial', () => {
    expect(profileInitial('élodie')).toBe('É');
    expect(profileInitial('  ')).toBe('?');
  });
});

import { describe, expect, it } from 'vitest';
import { normalizeDeviceCode } from './quick-connect';
describe('device approval codes', () => {
  it('accepts typed and scanned codes without allowing arbitrary redirect targets', () => {
    expect(normalizeDeviceCode(' abcd-1234 ')).toBe('ABCD1234');
    expect(normalizeDeviceCode('ABCD 1234')).toBe('ABCD1234');
    expect(normalizeDeviceCode('ABCD–1234')).toBe('ABCD1234');
    for (const input of ['123', '123456789', 'https://other.example', 'GGGG1234', '<script>', 'ABCD\n1234']) expect(normalizeDeviceCode(input)).toBeNull();
  });
});

import { describe, expect, it } from 'vitest';
import { isSafeHttpUrl } from './safe-url';

describe('isSafeHttpUrl', () => {
  it('accepts valid https and http URLs', () => {
    expect(isSafeHttpUrl('https://example.com')).toBe(true);
    expect(isSafeHttpUrl('http://localhost:3000/app')).toBe(true);
    expect(isSafeHttpUrl('https://github.com/user/repo#readme')).toBe(true);
  });

  it('rejects executable and non-http schemes (XSS vectors)', () => {
    expect(isSafeHttpUrl('javascript:alert(1)')).toBe(false);
    expect(isSafeHttpUrl('JavaScript:alert(document.cookie)')).toBe(false);
    expect(isSafeHttpUrl('data:text/html,<script>alert(1)</script>')).toBe(false);
    expect(isSafeHttpUrl('vbscript:msgbox(1)')).toBe(false);
    expect(isSafeHttpUrl('blob:https://example.com/uuid')).toBe(false);
    expect(isSafeHttpUrl('file:///etc/passwd')).toBe(false);
  });

  it('rejects empty, null, undefined or malformed inputs', () => {
    expect(isSafeHttpUrl('')).toBe(false);
    expect(isSafeHttpUrl(null)).toBe(false);
    expect(isSafeHttpUrl(undefined)).toBe(false);
    expect(isSafeHttpUrl('//protocol-relative.com')).toBe(false);
    expect(isSafeHttpUrl('/relative/path')).toBe(false);
    expect(isSafeHttpUrl('not a url')).toBe(false);
  });
});

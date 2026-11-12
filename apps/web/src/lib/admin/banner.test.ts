import { describe, expect, it } from 'vitest';
import { toBannerSrc } from './banner';

describe('toBannerSrc', () => {
  it('converts safe inline SVG into a data URI', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect fill="red"/></svg>';
    const result = toBannerSrc(svg);
    expect(result).toBe(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`);
  });

  it('rejects inline SVG containing XSS vectors', () => {
    expect(toBannerSrc('<svg><script>alert(1)</script></svg>')).toBeNull();
    expect(toBannerSrc('<svg><script href="data:,alert(1)"></script></svg>')).toBeNull();
    expect(toBannerSrc('<svg onload="alert(document.cookie)"></svg>')).toBeNull();
    expect(toBannerSrc('<svg onerror="alert(1)"></svg>')).toBeNull();
    expect(toBannerSrc('<svg><foreignObject><script>alert(1)</script></foreignObject></svg>')).toBeNull();
    expect(toBannerSrc('<svg><a href="javascript:alert(1)"><rect/></a></svg>')).toBeNull();
  });

  it('allows safe data:image/ URIs and rejects malicious ones', () => {
    expect(toBannerSrc('data:image/png;base64,iVBORw0KGgo=')).toBe('data:image/png;base64,iVBORw0KGgo=');
    expect(toBannerSrc('data:image/svg+xml;charset=utf-8,%3Csvg%3E%3Crect%2F%3E%3C%2Fsvg%3E')).toBe(
      'data:image/svg+xml;charset=utf-8,%3Csvg%3E%3Crect%2F%3E%3C%2Fsvg%3E'
    );
    expect(toBannerSrc('data:image/svg+xml;utf-8,<svg onload=alert(1)>')).toBeNull();
    expect(toBannerSrc('data:image/svg+xml;charset=utf-8,%3Csvg%20onload%3Dalert(1)%3E%3C%2Fsvg%3E')).toBeNull();
    expect(toBannerSrc(`data:image/svg+xml;base64,${btoa('<svg onload="alert(1)"></svg>')}`)).toBeNull();
    expect(toBannerSrc(`data:image/svg+xml;base64,${btoa('<svg><script>alert(1)</script></svg>')}`)).toBeNull();
    expect(toBannerSrc(`data:image/svg+xml;base64,${btoa('<svg><rect fill="blue"/></svg>')}`)).toBe(
      `data:image/svg+xml;base64,${btoa('<svg><rect fill="blue"/></svg>')}`
    );
    expect(toBannerSrc('data:image/svg+xml;base64,invalid-base-64!!!')).toBeNull();
    expect(toBannerSrc('data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==')).toBeNull();
  });

  it('allows safe http/https URLs and rejects unsafe schemes', () => {
    expect(toBannerSrc('https://example.com/banner.png')).toBe('https://example.com/banner.png');
    expect(toBannerSrc('http://localhost:8080/banner.jpg')).toBe('http://localhost:8080/banner.jpg');
    expect(toBannerSrc('javascript:alert(1)')).toBeNull();
    expect(toBannerSrc('vbscript:msgbox(1)')).toBeNull();
    expect(toBannerSrc('file:///etc/passwd')).toBeNull();
    expect(toBannerSrc('//evil.com/banner.png')).toBeNull();
  });

  it('returns null for empty, null or undefined input', () => {
    expect(toBannerSrc('')).toBeNull();
    expect(toBannerSrc('   ')).toBeNull();
    expect(toBannerSrc(null)).toBeNull();
    expect(toBannerSrc(undefined)).toBeNull();
  });
});

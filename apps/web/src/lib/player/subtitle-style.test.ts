import { describe, it, expect } from 'vitest';
import {
  DEFAULT_SUBTITLE_STYLE,
  sanitizeSubtitleStyle,
  subtitleStyleToCss,
  type SubtitleStyle
} from './subtitle-style';

describe('subtitle-style', () => {
  it('returns empty object when input is empty or invalid', () => {
    expect(sanitizeSubtitleStyle(null)).toEqual({});
    expect(sanitizeSubtitleStyle(undefined)).toEqual({});
    expect(sanitizeSubtitleStyle('invalid')).toEqual({});
    expect(sanitizeSubtitleStyle({ fontSize: 'huge' })).toEqual({});
  });

  it('sanitizes valid style attributes', () => {
    const input: SubtitleStyle = {
      fontSize: 'large',
      fontFamily: 'monospace',
      textColor: '#ffff00',
      backgroundColor: '#123456',
      backgroundOpacity: 80,
      textShadow: 'outline'
    };
    expect(sanitizeSubtitleStyle(input)).toEqual(input);
  });

  it('rejects invalid hex colors and out-of-range opacity', () => {
    const sanitized = sanitizeSubtitleStyle({
      textColor: 'red',
      backgroundColor: '#ZZZZZZ',
      backgroundOpacity: 150,
      textShadow: 'fancy'
    });
    expect(sanitized.textColor).toBeUndefined();
    expect(sanitized.backgroundColor).toBeUndefined();
    expect(sanitized.backgroundOpacity).toBeUndefined();
    expect(sanitized.textShadow).toBeUndefined();
  });

  it('maps transparent background when opacity is 0', () => {
    const css = subtitleStyleToCss({ backgroundOpacity: 0 });
    expect(css.backgroundColor).toBe('transparent');
  });

  it('maps correct rgba background when opacity > 0', () => {
    const css = subtitleStyleToCss({
      backgroundColor: '#ffffff',
      backgroundOpacity: 50
    });
    expect(css.backgroundColor).toBe('rgba(255, 255, 255, 0.5)');
  });

  it('uses default fallback values', () => {
    const css = subtitleStyleToCss();
    expect(css.color).toBe(DEFAULT_SUBTITLE_STYLE.textColor);
    expect(css.backgroundColor).toBe('transparent');
  });
});

import { isSafeHttpUrl } from '../safe-url';

const DANGEROUS_SVG_PATTERN =
  /<\s*(?:script|foreignObject|iframe|embed|object|use|animate|set|handler|listener|image|feImage)\b|<!(?:ENTITY|DOCTYPE)|on[a-z0-9_-]+\s*=|(?:\b(?:xlink:)?href\s*=)/i;

function isSafeSvg(raw: string): boolean {
  return !DANGEROUS_SVG_PATTERN.test(raw);
}

function parseSvgFromDataUri(uri: string): string | null {
  const commaIndex = uri.indexOf(',');
  if (commaIndex === -1) return null;
  const header = uri.slice(0, commaIndex).toLowerCase();
  const payload = uri.slice(commaIndex + 1);
  if (header.includes(';base64')) {
    try {
      return atob(payload.trim());
    } catch {
      return null;
    }
  }
  try {
    return decodeURIComponent(payload);
  } catch {
    return payload;
  }
}

/**
 * Validates and converts an extension banner into a safe image source URL.
 * Rejects XSS vectors including embedded scripts, event handlers, foreignObject,
 * and javascript: schemes in SVG or external URLs.
 */
export function toBannerSrc(banner?: string | null): string | null {
  if (!banner || typeof banner !== 'string') return null;
  const trimmed = banner.trim();
  if (trimmed.startsWith('<svg') || trimmed.includes('<svg')) {
    if (!isSafeSvg(trimmed)) {
      return null;
    }
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(trimmed)}`;
  }
  if (trimmed.startsWith('data:image/')) {
    if (trimmed.startsWith('data:image/svg+xml')) {
      const svg = parseSvgFromDataUri(trimmed);
      if (!svg || !isSafeSvg(svg)) {
        return null;
      }
      return trimmed;
    }
    if (/^data:image\/(?:png|jpeg|jpg|webp|gif|avif);base64,[a-z0-9+/=\s]+$/i.test(trimmed)) {
      return trimmed;
    }
    return null;
  }
  if ((trimmed.startsWith('http://') || trimmed.startsWith('https://')) && isSafeHttpUrl(trimmed)) {
    return trimmed;
  }
  return null;
}


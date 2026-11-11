/**
 * Validates that a string is a safe HTTP/HTTPS URL, neutralizing
 * javascript:, data:, vbscript:, and relative/malformed URL injection vectors.
 */
export function isSafeHttpUrl(url?: string | null): boolean {
  if (!url || typeof url !== 'string') return false;
  try {
    const parsed = new URL(url);
    return (parsed.protocol === 'https:' || parsed.protocol === 'http:') && Boolean(parsed.hostname);
  } catch {
    return false;
  }
}

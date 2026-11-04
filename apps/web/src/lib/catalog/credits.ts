import type { Work } from './api';

export interface CreditPerson {
  key: string;
  name: string;
  role: string;
  /** Core-served portrait, or '' when the initials avatar must be shown. */
  photoUrl: string;
}

/** Portraits are always proxied and cached by Core; third-party URLs are never loaded directly. */
export function corePortraitUrl(url: string | null | undefined): string {
  const value = (url ?? '').trim();
  return /^\/api\/v1\/works\/[^/]+\/people\/[0-9a-f]{32}\/image$/.test(value) ? value : '';
}

export function creditPeople(work: Pick<Work, 'actors' | 'directors' | 'writers'>): CreditPerson[] {
  const list: CreditPerson[] = [];
  const seen = new Set<string>();
  const photos = new Map<string, string>();

  for (const actor of work.actors ?? []) {
    const name = actor?.name?.trim();
    if (!name) continue;
    const photoUrl = corePortraitUrl(actor.imageUrl);
    if (photoUrl && !photos.has(name.toLowerCase())) photos.set(name.toLowerCase(), photoUrl);
    const key = `actor:${name.toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    list.push({ key, name, role: actor.role?.trim() || 'Distribution', photoUrl });
  }

  const crew = (names: string[] | undefined, kind: string, role: string) => {
    for (const raw of names ?? []) {
      const name = typeof raw === 'string' ? raw.trim() : '';
      if (!name) continue;
      const key = `${kind}:${name.toLowerCase()}`;
      if (seen.has(key)) continue;
      seen.add(key);
      // Crew credits carry no portrait; reuse the one of the same person in the cast, if any.
      list.push({ key, name, role, photoUrl: photos.get(name.toLowerCase()) ?? '' });
    }
  };
  crew(work.directors, 'director', 'Réalisateur');
  crew(work.writers, 'writer', 'Scénario');
  return list;
}

export function initials(name: string): string {
  const parts = name.split(/\s+/).filter(Boolean);
  const chosen = parts.length > 1 ? [parts[0], parts[parts.length - 1]] : parts;
  return chosen.map(part => Array.from(part)[0]?.toUpperCase() ?? '').join('');
}

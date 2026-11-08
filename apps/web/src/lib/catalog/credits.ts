import { tr } from '../i18n';
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

export function creditPeople(work: Pick<Work, 'actors' | 'directors' | 'writers' | 'crewCredits'>): CreditPerson[] {
  const list: CreditPerson[] = [];
  const seen = new Set<string>();

  for (const actor of work.actors ?? []) {
    const name = actor?.name?.trim();
    if (!name) continue;
    const photoUrl = corePortraitUrl(actor.imageUrl);
    const key = `actor:${name.toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    list.push({ key, name, role: actor.role?.trim() || tr('cast'), photoUrl });
  }

  const crew = (names: string[] | undefined, kind: string, role: string) => {
    for (const raw of names ?? []) {
      const name = typeof raw === 'string' ? raw.trim() : '';
      if (!name) continue;
      const key = `${kind}:${name.toLowerCase()}`;
      if (seen.has(key)) continue;
      seen.add(key);
      // Do not infer identity from a cast member's name; homonyms can be different people.
      const matches = (work.crewCredits ?? []).filter(member => {
        const job = member.role?.trim().toLowerCase();
        return member.name?.trim().toLowerCase() === name.toLowerCase()
          && (kind === 'director' ? job === 'director' : job === 'writer' || job === 'screenplay');
      });
      const photoUrl = matches.length === 1 ? corePortraitUrl(matches[0].imageUrl) : '';
      list.push({ key, name, role, photoUrl });
    }
  };
  crew(work.directors, 'director', tr('director'));
  crew(work.writers, 'writer', tr('screenplay'));
  return list;
}

export function initials(name: string): string {
  const parts = name.split(/\s+/).filter(Boolean);
  const chosen = parts.length > 1 ? [parts[0], parts[parts.length - 1]] : parts;
  return chosen.map(part => Array.from(part)[0]?.toUpperCase() ?? '').join('');
}

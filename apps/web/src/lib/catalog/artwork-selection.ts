import type { Session } from '../auth/session';
import type { Work } from './api';
import { preferences } from '../preferences';
import { onSessionGenerationChange } from '../auth/session';
import { artworkRotationPolicy, type ArtworkRotationPolicy } from './artwork-rotation-policy.svelte';

type Choice = { session: number; image: string; switchedAt?: number };
const history = new Map<string, Record<string, Choice>>();
const visits = new Map<string, string>();
onSessionGenerationChange(() => { history.clear(); visits.clear(); });

function visitSeed(current: Session | null): string {
  if (!current) return 'anonymous';
  const owner = current.user.id;
  const auth = hash(`artwork-auth:${current.csrfToken}`);
  const key = `${owner}:${auth}`;
  const cached = visits.get(key);
  if (cached) return cached;
  const storageKey = `media_artwork_visit_${owner}`;
  let seed = '';
  if (typeof sessionStorage !== 'undefined') {
    try {
      const raw = sessionStorage.getItem(storageKey);
      const stored = raw && raw.length < 1024 ? JSON.parse(raw) : null;
      if (stored?.auth === auth && typeof stored.seed === 'string' && stored.seed.length <= 80) seed = stored.seed;
    } catch { /* Optional visit cache. */ }
  }
  if (!seed) {
    seed = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}:${Math.random()}`;
    if (typeof sessionStorage !== 'undefined') {
      try { sessionStorage.setItem(storageKey, JSON.stringify({ auth, seed })); } catch { /* In-memory visit remains stable. */ }
    }
  }
  visits.set(key, seed);
  return seed;
}

function choose(work: Work, kind: string, candidates: NonNullable<Work['imageVariants']>, current: Session | null, policy: ArtworkRotationPolicy): string {
  const owner = current?.user.id ?? 'anonymous';
  const seed = `${owner}:${visitSeed(current)}`;
  const session = hash(`artwork-session:${seed}`);
  const storageKey = `media_artwork_choices_${owner}`;
  let choices = history.get(owner);
  if (!choices) {
    choices = {};
    if (typeof localStorage !== 'undefined') {
      try {
        const raw = localStorage.getItem(storageKey);
        const parsed: unknown = raw && raw.length < 131072 ? JSON.parse(raw) : {};
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
          for (const [key, value] of Object.entries(parsed).slice(-512)) {
            if (value && typeof value === 'object' && typeof value.session === 'number' && typeof value.image === 'string') choices[key] = value;
          }
        }
      } catch { /* Rotation also works without optional browser history. */ }
    }
    history.set(owner, choices);
  }
  const key = `${work.id}:${kind}`;
  const previous = choices[key];
  if (previous?.session === session && (policy.minHoursBetweenSwitches === 0 || !previous.switchedAt || Date.now() - previous.switchedAt < policy.minHoursBetweenSwitches * 3_600_000) && candidates.some(image => image.id === previous.image)) return candidates.find(image => image.id === previous.image)!.url;
  if (previous?.switchedAt && Date.now() - previous.switchedAt < policy.minHoursBetweenSwitches * 3_600_000) {
    const retained = candidates.find(image => image.id === previous.image);
    if (retained) return retained.url;
  }
  const previousIndex = candidates.findIndex(image => image.id === previous?.image);
  const pool = candidates.length > 1 && previousIndex >= 0 ? candidates.filter(image => image.id !== previous?.image) : candidates;
  const selected = policy.rotationMode === 'sequential' && previousIndex >= 0
    ? candidates[(previousIndex + 1) % candidates.length]
    : pool[hash(`${seed}:${work.id}:${kind}`) % pool.length];
  delete choices[key];
  choices[key] = { session, image: selected.id, switchedAt: Date.now() };
  while (Object.keys(choices).length > 512) delete choices[Object.keys(choices)[0]];
  if (typeof localStorage !== 'undefined') {
    try { localStorage.setItem(storageKey, JSON.stringify(choices)); } catch { /* Optional history only. */ }
  }
  return selected.url;
}

function hash(value: string): number {
  let result = 2166136261;
  for (let i = 0; i < value.length; i++) result = Math.imul(result ^ value.charCodeAt(i), 16777619);
  return result >>> 0;
}

/** Display choices only: authorization and image storage remain owned by Core. */
export function selectArtwork(work: Work | null | undefined, current: Session | null, languages: string[] = [preferences().locale ?? 'fr', 'en']): Record<string, string | null> {
  const policy = artworkRotationPolicy();
  const selected = { ...work?.artwork };
  if (!work) return selected;
  if (!selected.poster && work.posterUrl) selected.poster = work.posterUrl;
  const variants = work.imageVariants ?? [];
  for (const kind of new Set(variants.map(image => image.kind))) {
    let candidates = variants.filter(image => image.kind === kind && image.url);
    const manual = candidates.filter(image => image.origin === 'manual' || image.origin === 'legacy').sort((a, b) => a.position - b.position || a.id.localeCompare(b.id));
    if (manual.length) {
      selected[kind] = manual[0].url;
      continue;
    }
    const rotation = kind === 'poster' && policy.enabled
      && ((work.kind === 'film' && policy.enableMovies) || (work.kind === 'series' && policy.enableSeries));
    const selectionLanguages = rotation ? [policy.preferredLanguage, policy.fallbackLanguage] : languages;
    if (rotation) {
      candidates = candidates.filter(image => {
        const language = image.language?.toLowerCase();
        if (!language || language === 'und' || language === 'xx') return policy.includeUnknownLanguage;
        return policy.allowAnyLanguageFallback || selectionLanguages.some(choice => language === choice || language.split('-')[0] === choice.split('-')[0]);
      });
      if (!candidates.length) continue;
    }
    const languageRank = (language?: string | null) => {
      if (!language || language === 'und' || language === 'xx') return selectionLanguages.length * 2;
      const normalized = language.toLowerCase();
      for (let i = 0; i < selectionLanguages.length; i++) {
        const preferred = selectionLanguages[i].toLowerCase();
        if (normalized === preferred) return i * 2;
        if (normalized.split('-')[0] === preferred.split('-')[0]) return i * 2 + 1;
      }
      return selectionLanguages.length * 2 + 1;
    };
    const bestLanguage = Math.min(...candidates.map(image => languageRank(image.language)));
    candidates = candidates.filter(image => languageRank(image.language) === bestLanguage);
    // Stable identity ordering makes API ordering and shelf/detail navigation irrelevant.
    candidates.sort((a, b) => a.id.localeCompare(b.id));
    if (candidates.length) {
      selected[kind] = rotation ? choose(work, kind, candidates.slice(0, policy.poolSize), current, policy) : candidates[0].url;
    }
  }
  return selected;
}

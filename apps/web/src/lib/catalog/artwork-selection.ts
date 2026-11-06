import type { Session } from '../auth/session';
import type { Work } from './api';
import { preferences } from '../preferences';
import { onSessionGenerationChange } from '../auth/session';
import { artworkRotationPolicy, type ArtworkRotationPolicy } from './artwork-rotation-policy.svelte';

type Choice = { session: number; image: string; switchedAt?: number };
const history = new Map<string, Record<string, Choice>>();
/** Rich pools are renewed regularly, even during a long visit. */
const ROTATION_PERIOD_MS = 3_600_000;
/** Scene images carry no title and can rotate like posters; logos stay stable. */
const SCENE_KINDS = new Set(['backdrop', 'landscape', 'thumb']);
/** Lower-priority poster languages join the rotation until it has this many images. */
const MIN_ROTATION_POOL = 3;
// Each catalogue page visit draws a new selection; it stays stable while the
// page is open and when a work opened from it shows its details.
const newVisit = () => globalThis.crypto?.randomUUID?.() ?? `${Date.now()}:${Math.random()}`;
let pageVisit = newVisit();
onSessionGenerationChange(() => { history.clear(); pageVisit = newVisit(); });
export function renewArtworkVisit(): void { pageVisit = newVisit(); }

function choose(work: Work, kind: string, candidates: NonNullable<Work['imageVariants']>, current: Session | null, policy: ArtworkRotationPolicy): string {
  const owner = current?.user.id ?? 'anonymous';
  const seed = `${owner}:${pageVisit}`;
  const period = Math.floor(Date.now() / ROTATION_PERIOD_MS);
  const session = hash(`artwork-session:${seed}:${period}`);
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
  if (previous?.session === session && candidates.some(image => image.id === previous.image)) return candidates.find(image => image.id === previous.image)!.url;
  if (previous?.switchedAt && Date.now() - previous.switchedAt < policy.minHoursBetweenSwitches * 3_600_000) {
    const retained = candidates.find(image => image.id === previous.image);
    if (retained) return retained.url;
  }
  const previousIndex = candidates.findIndex(image => image.id === previous?.image);
  const pool = candidates.length > 1 && previousIndex >= 0 ? candidates.filter(image => image.id !== previous?.image) : candidates;
  const selected = policy.rotationMode === 'sequential' && previousIndex >= 0
    ? candidates[(previousIndex + 1) % candidates.length]
    : pool[hash(`${seed}:${period}:${work.id}:${kind}`) % pool.length];
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
    const pinned = candidates.filter(image => image.pinned).sort((a, b) => a.position - b.position || a.id.localeCompare(b.id));
    if (pinned.length) {
      selected[kind] = pinned[0].url;
      continue;
    }
    const isLibraryAllowed = policy.languageMode ? (work.kind === 'film' || work.kind === 'series') : policy.enabledLibraries !== undefined
      ? policy.enabledLibraries.includes(work.libraryId)
      : ((work.kind === 'film' && (policy.enableMovies ?? true)) || (work.kind === 'series' && (policy.enableSeries ?? true)));
    const rotates = policy.enabled && isLibraryAllowed && (kind === 'poster' || SCENE_KINDS.has(kind));
    const rotation = kind === 'poster' && rotates;
    if (kind === 'poster' && !rotation && !policy.languageMode) {
      const serverChoice = candidates.find(image => image.origin === 'provider' && image.position < 0);
      if (serverChoice) {
        selected[kind] = serverChoice.url;
        continue;
      }
    }
    const interfaceLanguage = languages[0] || 'fr';
    const original = work.originalLanguage?.trim();
    if (kind === 'poster' && policy.languageMode) {
      // Priority tiers: chosen language, then the other one (interface/original), textless,
      // English, everything else. Rotation fills its pool tier by tier so a single
      // localized poster still alternates with the next best ones.
      const knownOriginal = original && /^[a-zA-Z]{2,3}(?:-[a-zA-Z0-9]{2,8})*$/.test(original) ? original : undefined;
      const order = (policy.languageMode === 'original' ? [knownOriginal, interfaceLanguage] : [interfaceLanguage, knownOriginal])
        .filter((value): value is string => !!value).map(value => value.toLowerCase().split('-')[0]);
      const tier = (language?: string | null) => {
        const value = language?.toLowerCase().split('-')[0];
        if (!value || ['und', 'xx', '00', 'zxx'].includes(value)) return 2;
        const index = order.indexOf(value);
        if (index >= 0) return index;
        return value === 'en' ? 3 : 4;
      };
      const ranked = candidates.map(image => ({ image, rank: tier(image.language) }))
        .sort((a, b) => a.rank - b.rank || (a.image.language ?? '').localeCompare(b.image.language ?? '') || a.image.id.localeCompare(b.image.id));
      if (!ranked.length) continue;
      if (!rotation) { selected[kind] = ranked[0].image.url; continue; }
      const pool: typeof candidates = [];
      for (const rank of [...new Set(ranked.map(entry => entry.rank))]) {
        if (pool.length >= MIN_ROTATION_POOL) break;
        pool.push(...ranked.filter(entry => entry.rank === rank).map(entry => entry.image));
      }
      selected[kind] = choose(work, kind, pool.slice(0, Math.max(policy.poolSize, MIN_ROTATION_POOL)), current, policy);
      continue;
    }
    const selectionLanguages = rotation ? [policy.preferredLanguage, policy.fallbackLanguage] : languages;
    if (rotation) {
      candidates = candidates.filter(image => {
        const language = image.language?.toLowerCase();
        if (policy.languageMode) return true;
        if (!language || ['und', 'xx', '00', 'zxx'].includes(language)) return policy.includeUnknownLanguage;
        return policy.allowAnyLanguageFallback || selectionLanguages.some(choice => language === choice || language.split('-')[0] === choice.split('-')[0]);
      });
      if (!candidates.length) continue;
    }
    const languageRank = (language?: string | null) => {
      if (!language || ['und', 'xx', '00', 'zxx'].includes(language)) return selectionLanguages.length * 2;
      const normalized = language.toLowerCase();
      for (let i = 0; i < selectionLanguages.length; i++) {
        const preferred = selectionLanguages[i].toLowerCase();
        if (normalized === preferred) return i * 2;
        if (normalized.split('-')[0] === preferred.split('-')[0]) return i * 2 + 1;
      }
      const fallback = [interfaceLanguage, 'en'].findIndex(choice => normalized.split('-')[0] === choice.toLowerCase().split('-')[0]);
      return selectionLanguages.length * 2 + 1 + (fallback < 0 ? 2 : fallback);
    };
    // Text-free scene images avoid mixing an embedded foreign title with the local logo.
    const neutral = (language?: string | null) => !language || ['und', 'xx', '00', 'zxx'].includes(language.toLowerCase());
    if (kind === 'backdrop' || kind === 'landscape') {
      const textFree = candidates.filter(image => neutral(image.language));
      if (textFree.length) candidates = textFree;
    }
    if (kind === 'logo') {
      candidates = candidates.filter(image => neutral(image.language) || image.language?.toLowerCase().split('-')[0] === interfaceLanguage.toLowerCase().split('-')[0]);
      // A translated text title is preferable to a known foreign-language logo.
      if (!candidates.length) { selected[kind] = null; continue; }
    }
    const bestLanguage = Math.min(...candidates.map(image => languageRank(image.language)));
    candidates = candidates.filter(image => languageRank(image.language) === bestLanguage);
    // Stable identity ordering makes API ordering and shelf/detail navigation irrelevant.
    candidates.sort((a, b) => a.id.localeCompare(b.id));
    if (candidates.length) {
      selected[kind] = rotates ? choose(work, kind, candidates.slice(0, kind === 'poster' ? policy.poolSize : 24), current, policy) : candidates[0].url;
    }
  }
  return selected;
}

/** Conversion pure entre une œuvre et le formulaire d'édition des métadonnées (WorkTools / MetadataEditorModal). */
import type { Work } from './api';

export interface MetadataForm {
  title: string;
  plot: string;
  year: number | undefined;
  genres: string;
  tags: string;
  ids: string;
  age: number | undefined;
}

export interface MetadataPayload {
  title: string;
  plot: string;
  year: number | null;
  genres: string[];
  tags: string[];
  externalIds: unknown;
  minimumAge: number | null;
}

/** Liste séparée par des virgules -> éléments non vides. */
export function splitCsv(value: string): string[] {
  return value.split(',').map(s => s.trim()).filter(Boolean);
}

/** Valeurs initiales du formulaire (le texte « Média local indexé » n'est pas un vrai synopsis). */
export function metadataFormFromWork(work: Work): MetadataForm {
  return {
    title: work.title,
    plot: (work.plot ?? work.description ?? '').replace(/^Média local indexé$/i, ''),
    year: work.year ?? undefined,
    genres: (work.genres ?? []).join(', '),
    tags: (work.tags ?? []).join(', '),
    ids: JSON.stringify(work.externalIds ?? {}, null, 2),
    age: work.minimumAge ?? undefined
  };
}

/** Charge utile PUT ; `null` si le JSON des identifiants externes est invalide. */
export function metadataPayloadFromForm(form: MetadataForm): MetadataPayload | null {
  let externalIds;
  try {
    externalIds = JSON.parse(form.ids);
  } catch {
    return null;
  }
  return {
    title: form.title,
    plot: form.plot,
    year: form.year ?? null,
    genres: splitCsv(form.genres),
    tags: splitCsv(form.tags),
    externalIds,
    minimumAge: form.age ?? null
  };
}

// ============================================================================
// Éditeur complet de métadonnées multi-langue (inspiré de Jellyfin + Aether)
// ============================================================================

export type MetadataPersonType = 'actor' | 'director' | 'writer';

export interface MetadataPersonDraft {
  name: string;
  role: string;
  type: MetadataPersonType;
  imageUrl?: string | null;
  providerId?: string | null;
}

export interface MetadataTranslationDraft {
  title: string;
  sortTitle: string;
  tagline: string;
  plot: string;
  source?: string | null;
}

export interface AdminWorkMetadataResponse {
  workId: string;
  libraryId: string;
  kind: 'film' | 'series' | 'episode' | string;
  preferredMetadataLanguage?: string | null;
  metadataCountryCode?: string | null;
  title?: string | null;
  sortTitle?: string | null;
  originalTitle?: string | null;
  originalLanguage?: string | null;
  tagline?: string | null;
  plot?: string | null;
  year?: number | null;
  premiered?: string | null;
  addedAt?: number | null;
  rating?: number | null;
  criticRating?: number | null;
  mpaa?: string | null;
  minimumAge?: number | null;
  genres?: string[];
  tags?: string[];
  studios?: string[];
  countries?: string[];
  directors?: string[];
  writers?: string[];
  actors?: { name: string; role?: string | null; imageUrl?: string | null; thumbUrl?: string | null; thumb_url?: string | null }[];
  crewCredits?: { name: string; role?: string | null; imageUrl?: string | null; providerId?: string | null }[];
  externalIds?: Record<string, string>;
  collection?: { id?: string | null; name?: string | null } | null;
  locked?: boolean;
  lockedFields?: string[];
  translations?: Record<
    string,
    {
      locale?: string;
      source?: string;
      title?: string | null;
      sortTitle?: string | null;
      plot?: string | null;
      overview?: string | null;
      tagline?: string | null;
    }
  >;
}

export interface WorkMetadataDraft {
  activeLocale: string;
  defaultLocale: string;
  title: string;
  sortTitle: string;
  originalTitle: string;
  originalLanguage: string;
  tagline: string;
  plot: string;
  addedAtDate: string;
  premieredDate: string;
  year: string;
  rating: string;
  criticRating: string;
  mpaa: string;
  minimumAge: string;
  imdbId: string;
  tmdbId: string;
  tvdbId: string;
  collectionId: string;
  collectionName: string;
  extraExternalIds: { key: string; value: string }[];
  genres: string[];
  people: MetadataPersonDraft[];
  studios: string[];
  countries: string[];
  tags: string[];
  locked: boolean;
  lockedFields: string[];
  translations: Record<string, MetadataTranslationDraft>;
}

export const LOCKABLE_METADATA_FIELDS = [
  'title',
  'plot',
  'genres',
  'mpaa',
  'people',
  'countries',
  'studios',
  'tags'
] as const;

export type LockableMetadataField = (typeof LOCKABLE_METADATA_FIELDS)[number];

export const METADATA_LANGUAGE_OPTIONS: { code: string; label: string }[] = [
  { code: 'fr', label: 'Français (fr)' },
  { code: 'fr-ca', label: 'Français — Canada (fr-CA)' },
  { code: 'en', label: 'English (en)' },
  { code: 'de', label: 'Deutsch (de)' },
  { code: 'es', label: 'Español (es)' },
  { code: 'it', label: 'Italiano (it)' },
  { code: 'pt', label: 'Português (pt)' },
  { code: 'ja', label: '日本語 (ja)' },
  { code: 'ko', label: '한국어 (ko)' },
  { code: 'zh', label: '中文 (zh)' }
];

export const ORIGINAL_LANGUAGE_OPTIONS: { code: string; labelFr: string; labelEn: string }[] = [
  { code: '', labelFr: 'Non définie', labelEn: 'Not set' },
  { code: 'fr', labelFr: 'Français (fr)', labelEn: 'French (fr)' },
  { code: 'en', labelFr: 'Anglais (en)', labelEn: 'English (en)' },
  { code: 'ja', labelFr: 'Japonais (ja)', labelEn: 'Japanese (ja)' },
  { code: 'ko', labelFr: 'Coréen (ko)', labelEn: 'Korean (ko)' },
  { code: 'es', labelFr: 'Espagnol (es)', labelEn: 'Spanish (es)' },
  { code: 'de', labelFr: 'Allemand (de)', labelEn: 'German (de)' },
  { code: 'it', labelFr: 'Italien (it)', labelEn: 'Italian (it)' },
  { code: 'pt', labelFr: 'Portugais (pt)', labelEn: 'Portuguese (pt)' },
  { code: 'zh', labelFr: 'Chinois (zh)', labelEn: 'Chinese (zh)' },
  { code: 'ru', labelFr: 'Russe (ru)', labelEn: 'Russian (ru)' },
  { code: 'ar', labelFr: 'Arabe (ar)', labelEn: 'Arabic (ar)' },
  { code: 'hi', labelFr: 'Hindi (hi)', labelEn: 'Hindi (hi)' },
  { code: 'nl', labelFr: 'Néerlandais (nl)', labelEn: 'Dutch (nl)' },
  { code: 'pl', labelFr: 'Polonais (pl)', labelEn: 'Polish (pl)' },
  { code: 'sv', labelFr: 'Suédois (sv)', labelEn: 'Swedish (sv)' },
  { code: 'da', labelFr: 'Danois (da)', labelEn: 'Danish (da)' },
  { code: 'no', labelFr: 'Norvégien (no)', labelEn: 'Norwegian (no)' },
  { code: 'fi', labelFr: 'Finnois (fi)', labelEn: 'Finnish (fi)' },
  { code: 'tr', labelFr: 'Turc (tr)', labelEn: 'Turkish (tr)' }
];

export const PARENTAL_RATING_OPTIONS: string[] = [
  '',
  'TP',
  '-10',
  '-12',
  '-16',
  '-18',
  'G',
  'PG',
  'PG-13',
  'R',
  'NC-17',
  'TV-Y',
  'TV-Y7',
  'TV-G',
  'TV-PG',
  'TV-14',
  'TV-MA',
  'FSK 0',
  'FSK 6',
  'FSK 12',
  'FSK 16',
  'FSK 18'
];

export const CUSTOM_AGE_OPTIONS: { value: string; labelFr: string; labelEn: string }[] = [
  { value: '', labelFr: 'Hériter / Non défini', labelEn: 'Inherit / Unset' },
  { value: '0', labelFr: 'Tous publics (0+)', labelEn: 'All ages (0+)' },
  { value: '6', labelFr: '6 ans et +', labelEn: '6+' },
  { value: '7', labelFr: '7 ans et +', labelEn: '7+' },
  { value: '10', labelFr: '10 ans et +', labelEn: '10+' },
  { value: '12', labelFr: '12 ans et +', labelEn: '12+' },
  { value: '13', labelFr: '13 ans et +', labelEn: '13+' },
  { value: '14', labelFr: '14 ans et +', labelEn: '14+' },
  { value: '15', labelFr: '15 ans et +', labelEn: '15+' },
  { value: '16', labelFr: '16 ans et +', labelEn: '16+' },
  { value: '17', labelFr: '17 ans et +', labelEn: '17+' },
  { value: '18', labelFr: '18 ans et +', labelEn: '18+' },
  { value: '21', labelFr: '21 ans et +', labelEn: '21+' }
];

export function cleanPlaceholderPlot(raw?: string | null): string {
  if (!raw) return '';
  return raw.replace(/^(Média local indexé|Indexed local media)$/i, '').trim();
}

export function unixSecondsToDateInput(ts?: number | null): string {
  if (ts === null || ts === undefined || !Number.isFinite(ts) || ts <= 0) return '';
  const date = new Date(ts * 1000);
  if (Number.isNaN(date.getTime())) return '';
  const yyyy = date.getUTCFullYear();
  const mm = String(date.getUTCMonth() + 1).padStart(2, '0');
  const dd = String(date.getUTCDate()).padStart(2, '0');
  return `${yyyy}-${mm}-${dd}`;
}

export function dateInputToUnixSeconds(dateStr: string): number | null {
  const trimmed = dateStr.trim();
  if (!trimmed) return null;
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(trimmed);
  if (match) {
    const ms = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]), 12, 0, 0);
    return Number.isNaN(ms) ? null : Math.floor(ms / 1000);
  }
  const parsed = Date.parse(trimmed);
  return Number.isNaN(parsed) ? null : Math.floor(parsed / 1000);
}

export function normalizeDateInput(raw?: string | null): string {
  if (!raw) return '';
  const trimmed = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) return trimmed;
  const isoMatch = /^(\d{4}-\d{2}-\d{2})T/.exec(trimmed);
  if (isoMatch) return isoMatch[1];
  const frMatch = /^(\d{2})\/(\d{2})\/(\d{4})$/.exec(trimmed);
  if (frMatch) return `${frMatch[3]}-${frMatch[2]}-${frMatch[1]}`;
  return trimmed;
}

export function defaultSortTitle(title: string): string {
  return title
    .trim()
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^\p{L}\p{N}\s]+/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

export function createMetadataDraft(
  work: Work,
  remote?: AdminWorkMetadataResponse | null,
  uiLocale = 'fr'
): WorkMetadataDraft {
  const prefLang = (remote?.preferredMetadataLanguage || uiLocale || 'fr').toLowerCase();
  const defaultLocale = METADATA_LANGUAGE_OPTIONS.some(opt => opt.code === prefLang)
    ? prefLang
    : prefLang.split('-')[0] || 'fr';

  const baseTitle = (remote?.title ?? work.title ?? '').trim();
  const baseSortTitle = (remote?.sortTitle ?? work.sortTitle ?? '').trim() || defaultSortTitle(baseTitle);
  const baseTagline = (remote?.tagline ?? work.outline ?? '').trim();
  const basePlot = cleanPlaceholderPlot(remote?.plot ?? work.plot ?? work.description);

  const translations: Record<string, MetadataTranslationDraft> = {};
  const rawTranslations = remote?.translations ?? work.translations ?? {};
  for (const [locKey, entry] of Object.entries(rawTranslations)) {
    const normKey = locKey.toLowerCase();
    translations[normKey] = {
      title: (entry.title ?? '').trim(),
      sortTitle: (entry.sortTitle ?? '').trim(),
      tagline: (entry.tagline ?? '').trim(),
      plot: cleanPlaceholderPlot(entry.plot ?? entry.overview ?? ''),
      source: 'source' in entry ? entry.source ?? null : null
    };
  }

  if (!translations[defaultLocale]) {
    translations[defaultLocale] = {
      title: baseTitle,
      sortTitle: baseSortTitle,
      tagline: baseTagline,
      plot: basePlot,
      source: null
    };
  } else {
    if (!translations[defaultLocale].title) translations[defaultLocale].title = baseTitle;
    if (!translations[defaultLocale].sortTitle) translations[defaultLocale].sortTitle = baseSortTitle;
    if (!translations[defaultLocale].tagline) translations[defaultLocale].tagline = baseTagline;
    if (!translations[defaultLocale].plot) translations[defaultLocale].plot = basePlot;
  }

  const extIds = { ...(work.externalIds ?? {}), ...(remote?.externalIds ?? {}) };
  const imdbId = extIds.imdb ?? '';
  const tmdbId = extIds.tmdb ?? '';
  const tvdbId = extIds.tvdb ?? '';
  const extraExternalIds: { key: string; value: string }[] = [];
  for (const [k, v] of Object.entries(extIds)) {
    if (k !== 'imdb' && k !== 'tmdb' && k !== 'tvdb' && v) {
      extraExternalIds.push({ key: k, value: String(v) });
    }
  }

  const people: MetadataPersonDraft[] = [];
  const seenCrew = new Set<string>();
  const directors = remote?.directors ?? work.directors ?? [];
  const writers = remote?.writers ?? work.writers ?? [];
  const crewCredits = remote?.crewCredits ?? work.crewCredits ?? [];
  const actors = remote?.actors ?? work.actors ?? [];

  for (const credit of crewCredits) {
    if (!credit?.name?.trim()) continue;
    const roleLower = (credit.role ?? '').toLowerCase();
    const type: MetadataPersonType =
      roleLower.includes('writer') || roleLower.includes('scénar') || roleLower.includes('screenplay')
        ? 'writer'
        : 'director';
    const key = `${type}:${credit.name.trim().toLowerCase()}`;
    seenCrew.add(key);
    people.push({
      name: credit.name.trim(),
      role: credit.role?.trim() || (type === 'director' ? 'Director' : 'Writer'),
      type,
      imageUrl: credit.imageUrl ?? null,
      providerId: credit.providerId ?? null
    });
  }

  for (const dir of directors) {
    if (!dir?.trim()) continue;
    const key = `director:${dir.trim().toLowerCase()}`;
    if (seenCrew.has(key)) continue;
    seenCrew.add(key);
    people.push({
      name: dir.trim(),
      role: 'Director',
      type: 'director'
    });
  }

  for (const writer of writers) {
    if (!writer?.trim()) continue;
    const key = `writer:${writer.trim().toLowerCase()}`;
    if (seenCrew.has(key)) continue;
    seenCrew.add(key);
    people.push({
      name: writer.trim(),
      role: 'Writer',
      type: 'writer'
    });
  }

  for (const actor of actors) {
    if (!actor?.name?.trim()) continue;
    people.push({
      name: actor.name.trim(),
      role: (actor.role ?? '').trim(),
      type: 'actor',
      imageUrl: actor.imageUrl ?? actor.thumbUrl ?? actor.thumb_url ?? null
    });
  }

  const yearVal = remote?.year ?? work.year;
  const ratingVal = remote?.rating ?? work.rating;
  const criticVal = remote?.criticRating ?? work.criticRating;
  const minAgeVal = remote?.minimumAge !== undefined ? remote.minimumAge : work.minimumAge;
  const addedAtVal = remote?.addedAt ?? work.addedAt;
  const collection = remote?.collection ?? work.collection;

  const activeTr = translations[defaultLocale];

  return {
    activeLocale: defaultLocale,
    defaultLocale,
    title: activeTr?.title || baseTitle,
    sortTitle: activeTr?.sortTitle || baseSortTitle,
    originalTitle: (remote?.originalTitle ?? work.originalTitle ?? '').trim(),
    originalLanguage: (remote?.originalLanguage ?? work.originalLanguage ?? '').trim().toLowerCase(),
    tagline: activeTr?.tagline ?? baseTagline,
    plot: activeTr?.plot ?? basePlot,
    addedAtDate: unixSecondsToDateInput(addedAtVal),
    premieredDate: normalizeDateInput(remote?.premiered ?? work.premiered),
    year: yearVal !== null && yearVal !== undefined ? String(yearVal) : '',
    rating: ratingVal !== null && ratingVal !== undefined ? String(ratingVal) : '',
    criticRating: criticVal !== null && criticVal !== undefined ? String(criticVal) : '',
    mpaa: (remote?.mpaa ?? work.mpaa ?? '').trim(),
    minimumAge: minAgeVal !== null && minAgeVal !== undefined ? String(minAgeVal) : '',
    imdbId,
    tmdbId,
    tvdbId,
    collectionId: (collection?.id ?? '').trim(),
    collectionName: (collection?.name ?? '').trim(),
    extraExternalIds,
    genres: [...(remote?.genres ?? work.genres ?? [])].filter(Boolean),
    people,
    studios: [...(remote?.studios ?? work.studios ?? [])].filter(Boolean),
    countries: [...(remote?.countries ?? work.countries ?? [])].filter(Boolean),
    tags: [...(remote?.tags ?? work.tags ?? [])].filter(Boolean),
    locked: Boolean(remote?.locked ?? work.locked ?? false),
    lockedFields: [...(remote?.lockedFields ?? work.lockedFields ?? [])].filter(Boolean),
    translations
  };
}

/** Synchronise les champs localisés en cours d'édition dans la table `draft.translations[draft.activeLocale]`. */
export function syncActiveLocaleTranslation(draft: WorkMetadataDraft): WorkMetadataDraft {
  const loc = (draft.activeLocale || draft.defaultLocale || 'fr').toLowerCase();
  const existing = draft.translations[loc] ?? {
    title: '',
    sortTitle: '',
    tagline: '',
    plot: ''
  };
  draft.translations[loc] = {
    ...existing,
    title: draft.title.trim(),
    sortTitle: draft.sortTitle.trim(),
    tagline: draft.tagline.trim(),
    plot: draft.plot.trim()
  };
  return draft;
}

/** Change la langue d'édition active en sauvegardant le brouillon de la langue précédente. */
export function switchMetadataDraftLocale(
  draft: WorkMetadataDraft,
  nextLocale: string
): WorkMetadataDraft {
  syncActiveLocaleTranslation(draft);
  const norm = nextLocale.toLowerCase();
  const fallback = draft.translations[draft.defaultLocale] ?? {
    title: draft.title,
    sortTitle: draft.sortTitle,
    tagline: draft.tagline,
    plot: draft.plot
  };
  const target = draft.translations[norm] ?? {
    title: fallback.title,
    sortTitle: fallback.sortTitle,
    tagline: fallback.tagline,
    plot: ''
  };
  draft.activeLocale = norm;
  draft.title = target.title || fallback.title;
  draft.sortTitle = target.sortTitle || fallback.sortTitle;
  draft.tagline = target.tagline ?? '';
  draft.plot = target.plot ?? '';
  return draft;
}

/** Construit la charge utile `PUT /api/v1/admin/works/{work}/metadata` à partir du brouillon complet. */
export function buildMetadataPutPayload(draft: WorkMetadataDraft): Record<string, unknown> {
  syncActiveLocaleTranslation(draft);

  const defTr = draft.translations[draft.defaultLocale] ?? {
    title: draft.title,
    sortTitle: draft.sortTitle,
    tagline: draft.tagline,
    plot: draft.plot
  };

  const primaryTitle = (defTr.title || draft.title).trim();
  const primarySortTitle = (defTr.sortTitle || draft.sortTitle).trim();
  const primaryTagline = (defTr.tagline ?? draft.tagline).trim();
  const primaryPlot = (defTr.plot ?? draft.plot).trim();

  const externalIds: Record<string, string> = {};
  if (draft.imdbId.trim()) externalIds.imdb = draft.imdbId.trim();
  if (draft.tmdbId.trim()) externalIds.tmdb = draft.tmdbId.trim();
  if (draft.tvdbId.trim()) externalIds.tvdb = draft.tvdbId.trim();
  for (const extra of draft.extraExternalIds) {
    const k = extra.key.trim();
    const v = extra.value.trim();
    if (k && v) externalIds[k] = v;
  }

  const directors: string[] = [];
  const writers: string[] = [];
  const crewCredits: { name: string; role: string; imageUrl?: string | null; providerId?: string | null }[] = [];
  const actors: { name: string; role?: string | null; imageUrl?: string | null }[] = [];

  for (const person of draft.people) {
    const name = person.name.trim();
    if (!name) continue;
    if (person.type === 'director') {
      if (!directors.includes(name)) directors.push(name);
      crewCredits.push({
        name,
        role: person.role.trim() || 'Director',
        imageUrl: person.imageUrl ?? null,
        providerId: person.providerId ?? null
      });
    } else if (person.type === 'writer') {
      if (!writers.includes(name)) writers.push(name);
      crewCredits.push({
        name,
        role: person.role.trim() || 'Writer',
        imageUrl: person.imageUrl ?? null,
        providerId: person.providerId ?? null
      });
    } else {
      actors.push({
        name,
        role: person.role.trim() || null,
        imageUrl: person.imageUrl ?? null
      });
    }
  }

  const translationsPayload: Record<
    string,
    { title?: string | null; sortTitle?: string | null; tagline?: string | null; plot?: string | null }
  > = {};
  for (const [loc, tr] of Object.entries(draft.translations)) {
    const tTitle = tr.title.trim();
    const tSort = tr.sortTitle.trim();
    const tTagline = tr.tagline.trim();
    const tPlot = tr.plot.trim();
    if (tTitle || tSort || tTagline || tPlot) {
      translationsPayload[loc] = {
        title: tTitle || null,
        sortTitle: tSort || null,
        tagline: tTagline || null,
        plot: tPlot || null
      };
    }
  }

  const parsedYear = draft.year.trim() ? Number.parseInt(draft.year.trim(), 10) : null;
  const parsedRating = draft.rating.trim()
    ? Number.parseFloat(draft.rating.trim().replace(',', '.'))
    : null;
  const parsedCritic = draft.criticRating.trim()
    ? Number.parseFloat(draft.criticRating.trim().replace(',', '.'))
    : null;
  const parsedMinAge =
    draft.minimumAge.trim() !== '' ? Number.parseInt(draft.minimumAge.trim(), 10) : null;

  return {
    title: primaryTitle,
    sortTitle: primarySortTitle || null,
    originalTitle: draft.originalTitle.trim() || null,
    originalLanguage: draft.originalLanguage.trim() || null,
    tagline: primaryTagline || null,
    plot: primaryPlot,
    year: parsedYear !== null && Number.isFinite(parsedYear) ? parsedYear : null,
    premiered: draft.premieredDate.trim() || null,
    addedAt: dateInputToUnixSeconds(draft.addedAtDate),
    rating:
      parsedRating !== null && Number.isFinite(parsedRating)
        ? Math.min(10, Math.max(0, parsedRating))
        : null,
    criticRating:
      parsedCritic !== null && Number.isFinite(parsedCritic)
        ? Math.min(100, Math.max(0, parsedCritic))
        : null,
    mpaa: draft.mpaa.trim() || null,
    minimumAge: parsedMinAge !== null && Number.isFinite(parsedMinAge) ? parsedMinAge : null,
    genres: draft.genres.map(g => g.trim()).filter(Boolean),
    tags: draft.tags.map(t => t.trim()).filter(Boolean),
    studios: draft.studios.map(s => s.trim()).filter(Boolean),
    countries: draft.countries.map(c => c.trim()).filter(Boolean),
    directors,
    writers,
    actors,
    crewCredits,
    externalIds,
    collection: draft.collectionId.trim()
      ? {
          id: draft.collectionId.trim(),
          name: draft.collectionName.trim() || `Collection ${draft.collectionId.trim()}`
        }
      : null,
    locked: draft.locked,
    lockedFields: draft.lockedFields,
    translations: translationsPayload
  };
}

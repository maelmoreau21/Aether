import { get, writable } from 'svelte/store';
import { getSessionGeneration, onSessionGenerationChange, session } from '../auth/session';

export type RequestKind = 'film' | 'series' | 'artist' | 'album' | 'track';
export const kindLabels: Record<RequestKind, string> = { film: 'Film', series: 'Série', artist: 'Artiste', album: 'Album', track: 'Titre musical' };
export const statusLabels: Record<string, string> = {
  pending: 'En attente de validation', approved: 'Approuvée', queued: 'À transmettre', submitted: 'Transmise',
  downloading: 'En acquisition', acquiring: 'En acquisition', processing: 'En acquisition', imported: 'Importée', available: 'Disponible dans votre médiathèque',
  partially_available: 'Partiellement disponible', partial: 'Partiellement disponible', suspended: 'Suspendue après import', rejected: 'Refusée', failed: 'Échouée', cancelled: 'Annulée',
  open: 'Ouvert', in_progress: 'En cours', resolved: 'Résolu', closed: 'Fermé sans correction'
};
export const categoryLabels: Record<string, string> = { video: 'Vidéo', audio: 'Audio', subtitles: 'Sous-titres', metadata: 'Métadonnées', missing: 'Fichier manquant', other: 'Autre' };

export interface Features { requests: boolean; reports: boolean }
export const features = writable<Features>({ requests: false, reports: false });
let pending: Promise<Features> | undefined;
let loadedAt = 0;
onSessionGenerationChange(() => { features.set({ requests: false, reports: false }); pending = undefined; loadedAt = 0; });

const errors: Record<string, string> = {
  extension_unavailable: 'Cette extension est désactivée ou indisponible.', feature_unavailable: 'Cette extension est désactivée ou indisponible.',
  request_limit: 'Votre limite de demandes en attente est atteinte.', quota_exceeded: 'Votre limite de demandes est atteinte.',
  duplicate_request: 'Une demande existe déjà pour ce contenu.', invalid_transition: 'Cette action ne correspond plus à l’état du dossier. Actualisez la page.',
  csrf_required: 'Votre session a changé. Actualisez la page.', authentication_required: 'Reconnectez-vous pour continuer.',
  lidarr_track_unsupported: 'Ce titre n’est pas disponible pour acquisition dans Lidarr.',
  unsupported_track: 'Ce titre ne peut pas être acquis seul dans Lidarr.', remote_unavailable: 'Le serveur distant est indisponible.',
  connection_unavailable: 'La connexion au serveur est indisponible.', not_found: 'Dossier introuvable ou inaccessible.'
};
export function errorMessage(code: string | undefined, status?: number): string {
  if (code && errors[code]) return errors[code];
  if (status === 401) return 'Votre session a expiré. Reconnectez-vous.';
  if (status === 403) return 'Cette action n’est pas autorisée.';
  if (status === 404) return 'Dossier ou extension introuvable.';
  if (status === 409) return 'Le dossier a changé ou une demande existe déjà. Actualisez la page.';
  return code ? `Opération impossible (${code}).` : 'Opération impossible. Vérifiez votre connexion et réessayez.';
}
export async function applicationRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: init.signal ?? AbortSignal.timeout(30000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current?.csrfToken ?? '', ...init.headers }
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; code?: string } | null;
    throw new Error(errorMessage(detail?.error ?? detail?.code, response.status));
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}
export async function loadFeatures(force = false): Promise<Features> {
  if (pending) return pending;
  if (!force && loadedAt && Date.now() - loadedAt < 15000) return get(features);
  const generation = getSessionGeneration();
  const task = applicationRequest<Features>('/api/v1/extensions/features').then(result => {
    const safe = { requests: result.requests === true, reports: result.reports === true };
    if (generation === getSessionGeneration()) { features.set(safe); loadedAt = Date.now(); }
    return safe;
  });
  pending = task;
  try { return await task; } finally { if (pending === task) pending = undefined; }
}
export interface SearchResult { kind: RequestKind; title: string; externalId: string; externalProvider: string; serverId?: string; year?: number; summary?: string }
export interface MediaRequest extends SearchResult { id: string; status: string; createdAt: number; updatedAt?: number; userId?: string; requestedBy?: string; error?: string; errorCode?: string; seasons?: number[]; episodes?: { season: number; episode: number }[]; history?: { status?: string; action?: string; createdAt: number; message?: string }[] }
export interface RemoteServer { id: string; name: string; kind: 'radarr' | 'sonarr' | 'lidarr'; baseUrl: string; hasApiKey?: boolean; libraryId: string; rootFolder: string; qualityProfileId: number; metadataProfileId?: number; isDefault: boolean; enabled: boolean }
export interface ReportContext { engine?: string; errorCode?: string; audioTrackId?: string; subtitleTrackId?: string }
export interface ReportInput { category: string; description: string; workId?: string; trackId?: string; assetId?: string; positionSeconds?: number; endSeconds?: number; context?: ReportContext }
export interface MediaReport extends ReportInput { id: string; status: string; createdAt: number; title?: string; userId?: string; comments?: { id?: string; message: string; createdAt: number; author?: string }[]; history?: { status?: string; action?: string; createdAt: number }[] }
export function dateLabel(value: number | string | undefined): string {
  if (!value) return '—';
  const date = new Date(typeof value === 'number' ? value * 1000 : value);
  return Number.isNaN(date.valueOf()) ? '—' : date.toLocaleString('fr-FR');
}
export function parseSelection(seasons: string, episodes: string): { seasons: number[]; episodes: { season: number; episode: number }[] } {
  if (seasons.trim() && seasons.split(',').some(item => !item.trim())) throw new Error('Indiquez des numéros de saisons séparés par des virgules.');
  const selectedSeasons = seasons.trim() ? seasons.split(',').map(item => Number(item.trim())) : [];
  if (selectedSeasons.some(value => !Number.isInteger(value) || value < 0 || value > 1000)) throw new Error('Indiquez des numéros de saisons séparés par des virgules.');
  const selectedEpisodes = episodes.trim() ? episodes.split(',').map(item => {
    const match = /^\s*(\d{1,4})\s*[:x]\s*(\d{1,4})\s*$/.exec(item);
    if (!match || Number(match[2]) < 1) throw new Error('Indiquez les épisodes sous la forme 1:3, 1:4 (saison:épisode).');
    return { season: Number(match[1]), episode: Number(match[2]) };
  }) : [];
  return { seasons: [...new Set(selectedSeasons)], episodes: selectedEpisodes.filter((value, index, all) => all.findIndex(other => other.season === value.season && other.episode === value.episode) === index) };
}
export function reportPayload(input: ReportInput): ReportInput {
  if (!input.description.trim()) throw new Error('Décrivez le problème rencontré.');
  if (!input.workId && !input.trackId) throw new Error('Sélectionnez un média depuis sa fiche ou son lecteur.');
  for (const value of [input.positionSeconds, input.endSeconds]) if (value !== undefined && (!Number.isFinite(value) || value < 0)) throw new Error('La position doit être un nombre positif.');
  if (input.endSeconds !== undefined && (input.positionSeconds === undefined || input.endSeconds < input.positionSeconds)) throw new Error('La fin doit suivre le début du problème.');
  const context = input.context ? Object.fromEntries(['engine', 'errorCode', 'audioTrackId', 'subtitleTrackId'].filter(key => typeof input.context?.[key as keyof ReportContext] === 'string').map(key => [key, input.context?.[key as keyof ReportContext]])) : undefined;
  return { category: input.category, description: input.description.trim(), workId: input.workId, trackId: input.trackId, assetId: input.assetId, positionSeconds: input.positionSeconds, endSeconds: input.endSeconds, context };
}

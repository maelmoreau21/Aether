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
  extension_disabled: 'Cette extension est désactivée ou indisponible.',
  extension_busy: 'Une autre opération est en cours. Réessayez dans un instant.',
  library_permission_denied: 'Vous n’avez plus accès à la bibliothèque concernée.',
  report_limit_reached: 'Votre limite de signalements ouverts est atteinte.',
  comment_limit_reached: 'Ce signalement a atteint sa limite de commentaires.',
  track_not_supported: 'Demande échouée : ce titre n’est pas disponible pour acquisition dans Lidarr.',
  single_monitoring_conflict: 'Ce single possède déjà une surveillance incompatible dans Lidarr. Vérifiez son édition avant de reprendre la demande.',
  single_plan_unverified: 'L’identité exacte du single n’a pas pu être vérifiée. Aucun album complet ne sera acquis à sa place.',
  remote_content_not_found: 'Ce contenu n’existe pas dans le catalogue du serveur choisi.',
  request_quota_exceeded: 'Votre limite de demandes en attente est atteinte.',
  request_action_not_allowed: 'Cette action ne correspond plus à l’état de la demande. Actualisez la page.',
  external_metadata_parental_unverified: 'La classification de ce contenu externe ne peut pas être vérifiée pour votre compte.',
  acquisition_server_not_configured: 'Un administrateur doit configurer une destination pour ce type de média.',
  acquisition_server_incompatible: 'La destination choisie est désactivée ou incompatible avec ce type de média.',
  server_has_request_history_disable_instead: 'Cette connexion possède un historique. Désactivez-la pour conserver le suivi des demandes.',
  connection_address_changed: 'L’adresse réseau du serveur a changé. Un administrateur doit réenregistrer la connexion.',
  api_key_required: 'Saisissez à nouveau la clé API après un changement de service ou d’adresse.',
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
  const generation = getSessionGeneration();
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: init.signal ?? AbortSignal.timeout(30000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current?.csrfToken ?? '', ...init.headers }
  });
  if (generation !== getSessionGeneration()) throw new Error('Votre session a changé. Actualisez la page.');
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; code?: string } | null;
    throw new Error(errorMessage(detail?.error ?? detail?.code, response.status));
  }
  const result = response.status === 204 ? undefined as T : await response.json() as T;
  if (generation !== getSessionGeneration()) throw new Error('Votre session a changé. Actualisez la page.');
  return result;
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
export interface RequestSelection { seasons?: number[]; episodes?: { season: number; episode: number }[]; albumIds?: string[]; monitorFuture?: boolean; serverId?: string }
export interface RequestEvent { status?: string; action?: string; createdAt: number; message?: string; code?: string }
export interface MediaRequest extends SearchResult { id: string; status: string; createdAt: number; updatedAt?: number; userId?: string; requestedBy?: string; error?: string; errorCode?: string; selection?: RequestSelection; events?: RequestEvent[]; authorLabel?: string; sourceStatus?: string }
export interface RemoteServer { id: string; name: string; kind: 'radarr' | 'sonarr' | 'lidarr'; baseUrl: string; hasApiKey?: boolean; libraryId: string; rootFolder: string; qualityProfileId: number; metadataProfileId?: number; isDefault: boolean; enabled: boolean }
export interface ReportContext { engine?: string; errorCode?: string; audioTrackId?: string; subtitleTrackId?: string }
export interface ReportInput { category: string; description: string; workId?: string; trackId?: string; assetId?: string; positionSeconds?: number; endSeconds?: number; context?: ReportContext }
export interface MediaReport extends ReportInput { id: string; status: string; createdAt: number; title?: string; userId?: string; authorLabel?: string; comments?: { id?: string; message: string; createdAt: number; author?: string; authorLabel?: string; userId?: string }[]; history?: { status?: string; action?: string; createdAt: number }[] }
export function requestPayload(result: SearchResult, selection: RequestSelection): SearchResult & RequestSelection {
  return { kind: result.kind, title: result.title, externalId: result.externalId, externalProvider: result.externalProvider,
    ...(selection.serverId || result.serverId ? { serverId: selection.serverId || result.serverId } : {}),
    seasons: selection.seasons ?? [], episodes: selection.episodes ?? [], albumIds: selection.albumIds ?? [], monitorFuture: selection.monitorFuture ?? false };
}
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

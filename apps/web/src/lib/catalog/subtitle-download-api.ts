export interface SubtitleResult {
  id: string;
  language: string;
  label: string;
  forced: boolean;
  sdh: boolean;
  match: string;
  downloadCount?: number;
}

export interface SubtitleDownload {
  id: string;
  providerId: string;
  fileId: string;
  language: string;
  label: string;
  forced: boolean;
  sdh: boolean;
  status: string;
  errorCode?: string;
}

export interface SubtitleInventory {
  providers: { id: string; name: string }[];
  downloads: SubtitleDownload[];
  enabled: boolean;
  unavailableCode?: string;
  quotas?: { providerId: string; remaining: number | null; resetTimeUtc: string | null; nextAllowedAt: number }[];
}

export function subtitleErrorText(code: string, status = 0): string {
  if (status === 401) return 'Votre session a expiré. Connectez-vous à nouveau.';
  if (status === 403) return 'Action non autorisée pour ce compte ou ce média.';
  if (status === 429 || /quota|rate_limit/.test(code)) return 'Quota ou limite de requêtes atteint. Attendez avant de réessayer manuellement.';
  if (/hash_required|hash_unavailable/.test(code)) return 'Cette bibliothèque exige une correspondance par hash, indisponible pour ce parcours. Vérifiez sa politique de sous-titres.';
  if (/download_disabled|subtitles_disabled/.test(code)) return 'Le téléchargement de sous-titres est désactivé pour cette bibliothèque.';
  if (/auth|credential|configuration/.test(code)) return 'Vérifiez la configuration et les identifiants du fournisseur dans Extensions.';
  if (/timeout|deadline/.test(code)) return 'Le fournisseur a dépassé le délai. Réessayez manuellement plus tard.';
  if (/already_downloaded|duplicate|already_exists/.test(code)) return 'Ce sous-titre est déjà téléchargé pour cette copie.';
  if (/attempt_exists/.test(code)) return 'Une tentative existe déjà. Actualisez son état ; supprimez une tentative interrompue ou échouée avant de réessayer manuellement.';
  if (/stale|media_changed|source_changed/.test(code)) return 'Le média a changé. Supprimez le téléchargement périmé puis lancez une nouvelle recherche.';
  if (/search_expired|search_not_found/.test(code)) return 'Cette recherche a expiré. Lancez une nouvelle recherche.';
  if (status === 404) return 'La copie ou le téléchargement est introuvable.';
  return 'Les sous-titres sont indisponibles. Vérifiez les réglages de la bibliothèque et du fournisseur.';
}

export function subtitleMatchText(match: string): string {
  if (['identity', 'imdb', 'external_id', 'externalId'].includes(match)) return 'Correspondance par identifiant ; synchronisation non garantie.';
  if (['title', 'query', 'title_search'].includes(match)) return 'Recherche par titre ; correspondance et synchronisation à vérifier.';
  if (match === 'hash') return 'Correspondance par hash annoncée par le fournisseur.';
  return 'Correspondance non vérifiée.';
}

export async function subtitleRequest<T>(
  path: string,
  signal: AbortSignal,
  csrfToken: string,
  method = 'GET',
  body?: unknown
): Promise<T> {
  const response = await fetch(path, {
    method,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: method === 'GET' ? {} : { 'content-type': 'application/json', 'x-csrf-token': csrfToken },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.any([signal, AbortSignal.timeout(path.endsWith('/download') ? 90000 : 70000)])
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}));
    const code = typeof payload.pluginError === 'string' ? payload.pluginError : typeof payload.error === 'string' ? payload.error : typeof payload.code === 'string' ? payload.code : '';
    throw new Error(subtitleErrorText(code, response.status));
  }
  return response.status === 204 ? undefined as T : await response.json() as T;
}

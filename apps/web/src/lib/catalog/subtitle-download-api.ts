import { tr } from '../i18n';
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
  if (status === 401) return tr('catalog_your_session_has_expired_sign_in');
  if (status === 403) return tr('action_not_allowed_for_this_account');
  if (status === 429 || /quota|rate_limit/.test(code)) return tr('quota_or_request_limit_reached_wait');
  if (/hash_required|hash_unavailable/.test(code)) return tr('this_library_requires_a_hash_match');
  if (/download_disabled|subtitles_disabled/.test(code)) return tr('subtitle_download_is_disabled_for_this');
  if (/auth|credential|configuration/.test(code)) return tr('check_the_provider_s_configuration_and');
  if (/timeout|deadline/.test(code)) return tr('the_provider_timed_out_try_again');
  if (/already_downloaded|duplicate|already_exists/.test(code)) return tr('this_subtitle_is_already_downloaded_for');
  if (/attempt_exists/.test(code)) return tr('an_attempt_already_exists_refresh_its');
  if (/stale|media_changed|source_changed/.test(code)) return tr('the_media_has_changed_delete_the');
  if (/search_expired|search_not_found/.test(code)) return tr('this_search_has_expired_start_a');
  if (status === 404) return tr('the_copy_or_download_was_not');
  return tr('subtitles_are_unavailable_check_the_library');
}

export function subtitleMatchText(match: string): string {
  if (['identity', 'imdb', 'external_id', 'externalId'].includes(match)) return tr('match_by_identifier_synchronisation_not');
  if (['title', 'query', 'title_search'].includes(match)) return tr('title_search_match_and_synchronisation_to');
  if (match === 'hash') return tr('hash_match_reported_by_the_provider');
  return tr('match_not_verified');
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

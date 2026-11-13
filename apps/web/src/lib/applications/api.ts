import { get, writable } from 'svelte/store';
import { getSessionGeneration, onSessionGenerationChange, session } from '../auth/session';

import { tr, currentFormat, translatedLabels } from '../i18n';
export type RequestKind = 'film' | 'series' | 'artist' | 'album' | 'track';
export const kindLabels: Readonly<Record<RequestKind, string>> = translatedLabels({ film: 'kind_movie', series: 'kind_series', artist: 'artist', album: 'album', track: 'music_track' });
export const statusLabels: Readonly<Record<string, string>> = translatedLabels({ pending: 'awaiting_approval', approved: 'approved', queued: 'to_send', submitted: 'sent', downloading: 'being_acquired', acquiring: 'being_acquired', processing: 'being_acquired', imported: 'imported', available: 'available_in_your_library', partially_available: 'partially_available', partial: 'partially_available', suspended: 'suspended_after_import', rejected: 'rejected', failed: 'applications_failed', cancelled: 'sched_outcome_cancelled', open: 'applications_open', in_progress: 'stats_kpi_active', resolved: 'resolved', closed: 'closed_without_a_fix' });
export const categoryLabels: Readonly<Record<string, string>> = translatedLabels({ video: 'applications_video', audio: 'audio', subtitles: 'report_category_subtitles', metadata: 'metadata', missing: 'report_category_missing', other: 'report_category_other' });

export interface Features { requests: boolean; reports: boolean }
export const features = writable<Features>({ requests: false, reports: false });
let pending: Promise<Features> | undefined;
let loadedAt = 0;
onSessionGenerationChange(() => { features.set({ requests: false, reports: false }); pending = undefined; loadedAt = 0; });

const errors: Readonly<Record<string, string>> = translatedLabels({
  extension_disabled: 'err_extension_unavailable', extension_unavailable: 'err_extension_unavailable', feature_unavailable: 'err_extension_unavailable',
  extension_busy: 'err_extension_busy',
  library_permission_denied: 'err_library_permission_denied',
  report_limit_reached: 'err_report_limit_reached',
  comment_limit_reached: 'err_comment_limit_reached',
  track_not_supported: 'err_track_not_supported',
  single_monitoring_conflict: 'err_single_monitoring_conflict',
  single_plan_unverified: 'err_single_plan_unverified',
  remote_content_not_found: 'err_remote_content_not_found',
  request_quota_exceeded: 'err_request_quota', request_limit: 'err_request_quota', quota_exceeded: 'err_quota_exceeded',
  request_action_not_allowed: 'err_state_changed', invalid_transition: 'err_state_changed', request_changed: 'err_state_changed', report_changed: 'err_state_changed',
  external_metadata_parental_unverified: 'err_parental_unverified',
  acquisition_server_not_configured: 'err_server_not_configured',
  acquisition_server_incompatible: 'err_server_incompatible',
  server_has_request_history_disable_instead: 'err_server_history',
  server_has_active_requests: 'err_server_active_requests',
  request_already_dispatched: 'err_request_dispatched',
  connection_address_changed: 'err_connection_address_changed',
  api_key_required: 'err_api_key_required',
  duplicate_request: 'err_duplicate_request', request_already_exists: 'err_duplicate_request',
  csrf_required: 'err_csrf_required', authentication_required: 'err_authentication_required',
  lidarr_track_unsupported: 'err_lidarr_track', unsupported_track: 'err_unsupported_track',
  remote_unavailable: 'err_remote_unavailable', connection_unavailable: 'err_connection_unavailable', not_found: 'err_not_found',
  invalid_reject_reason: 'err_invalid_reject_reason', empty_selection: 'err_empty_selection', invalid_request_selection: 'err_empty_selection', invalid_priority: 'err_invalid_priority',
  request_permission_denied: 'err_request_permission', request_period_quota_exceeded: 'err_request_period_quota',
  invalid_request_policy: 'err_invalid_policy', policy_target_is_profile: 'err_policy_profile', group_not_found: 'err_not_found', user_not_found: 'err_not_found',
  invalid_quality_profile: 'err_invalid_quality_profile'
});
export function errorMessage(code: string | null | undefined, status?: number): string {
  if (code && code in errors) return errors[code];
  if (status === 401) return tr('your_session_has_expired_sign_in');
  if (status === 403) return tr('this_action_is_not_allowed');
  if (status === 404) return tr('case_or_extension_not_found');
  if (status === 409) return tr('the_case_has_changed_or_a');
  return code ? tr('operation_not_possible', { code }) : tr('operation_not_possible_check_your_connection');
}
export async function applicationRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  const generation = getSessionGeneration();
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: init.signal ?? AbortSignal.timeout(30000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current?.csrfToken ?? '', ...init.headers }
  });
  if (generation !== getSessionGeneration()) throw new Error(tr('your_session_has_changed_refresh_the'));
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; code?: string } | null;
    throw new Error(errorMessage(detail?.error ?? detail?.code, response.status));
  }
  const result = response.status === 204 ? undefined as T : await response.json() as T;
  if (generation !== getSessionGeneration()) throw new Error(tr('your_session_has_changed_refresh_the'));
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
export interface SeasonInfo { number: number; episodes?: number | null }
export interface SearchResult {
  kind: RequestKind; title: string; externalId: string; externalProvider: string; serverId?: string; year?: number | null; summary?: string | null;
  genres?: string[]; runtime?: number | null; network?: string | null; artist?: string | null; album?: string | null; seasons?: SeasonInfo[];
  onServer?: boolean; inLibrary?: { workId: string }; request?: { status: string; mine: boolean; id?: string | null };
  /** Artwork of a title that is not in the library yet: a fixed provider and a plain path, served by Aether. */
  poster?: { src: 'tmdb' | 'tvdb'; path: string } | null;
  /** What the caller's rights say: may request it, and whether it is approved without an administrator. */
  canRequest?: boolean; autoApprove?: boolean;
}
export interface QualityProfile { id: number; name: string }
export interface RequestSelection { seasons?: number[]; episodes?: { season: number; episode: number }[]; albumIds?: string[]; monitorFuture?: boolean; serverId?: string; qualityProfileId?: number }
export interface RequestEvent { status?: string; action?: string; createdAt: number; message?: string; code?: string | null; actorLabel?: string | null }
export interface DownloadProgress { percent: number; totalBytes: number; remainingBytes: number; status: string; timeleft?: string | null; items: number }
export interface MediaRequest extends SearchResult { id: string; status: string; createdAt: number; updatedAt?: number; userId?: string; own?: boolean; workId?: string | null; error?: string; errorCode?: string | null; selection?: RequestSelection; progress?: DownloadProgress | null; events?: RequestEvent[]; authorLabel?: string | null; sourceStatus?: string | null }
export interface RemoteServer { id: string; name: string; kind: 'radarr' | 'sonarr' | 'lidarr'; baseUrl: string; hasApiKey?: boolean; libraryId: string; rootFolder: string; qualityProfileId: number; qualityProfiles?: QualityProfile[]; metadataProfileId?: number; isDefault: boolean; enabled: boolean }
export interface ReportContext { engine?: string; errorCode?: string; audioTrackId?: string; subtitleTrackId?: string }
export interface ReportInput { category: string; description: string; workId?: string; trackId?: string; assetId?: string; positionSeconds?: number; endSeconds?: number; context?: ReportContext }
export interface ReportSubject { kind: string; title: string; year?: number | null; artist?: string; album?: string; workId?: string; trackId?: string; image?: string }
export interface ReportComment { id?: string; message: string; createdAt: number; authorLabel?: string | null; staff?: boolean; own?: boolean }
export interface MediaReport extends ReportInput {
  id: string; status: string; createdAt: number; updatedAt?: number; userId?: string; authorLabel?: string | null; subject?: ReportSubject;
  priority?: Priority; triage?: { code?: string | null; checks?: string[] }; comments?: ReportComment[];
  history?: { kind?: string; status?: string; createdAt: number; actorLabel?: string | null }[];
}
export type Priority = 'high' | 'normal' | 'low';
export type Tone = 'neutral' | 'info' | 'warn' | 'ok' | 'err';

/** Visual weight of a status badge, shared by requests and reports. */
export function statusTone(status: string): Tone {
  if (['available', 'resolved'].includes(status)) return 'ok';
  if (['pending', 'suspended', 'partial', 'partially_available', 'open'].includes(status)) return 'warn';
  if (['failed'].includes(status)) return 'err';
  if (['approved', 'queued', 'submitted', 'acquiring', 'downloading', 'processing', 'imported', 'in_progress'].includes(status)) return 'info';
  return 'neutral';
}
/** Ordered steps a request goes through until it is playable in Aether. */
export const requestSteps = ['pending', 'approved', 'submitted', 'acquiring', 'available'] as const;
/** Index of the step reached, or -1 when the request left the normal path. */
export function requestStep(status: string): number {
  const aliases: Record<string, (typeof requestSteps)[number]> = { queued: 'approved', downloading: 'acquiring', processing: 'acquiring', imported: 'acquiring', partial: 'acquiring', partially_available: 'acquiring' };
  return requestSteps.indexOf(aliases[status] ?? status as (typeof requestSteps)[number]);
}
/** Status filter groups understood by `GET /requests?status=`. */
export const requestGroups = ['all', 'active', 'pending', 'done', 'attention', 'closed'] as const;
export type RequestGroup = (typeof requestGroups)[number];
export function groupCount(counts: Record<string, number>, group: RequestGroup): number {
  const members: Record<RequestGroup, string[]> = {
    all: Object.keys(counts), active: ['pending', 'approved', 'submitted', 'acquiring', 'imported', 'partial'], pending: ['pending'],
    done: ['available'], attention: ['failed', 'suspended'], closed: ['rejected', 'cancelled']
  };
  return members[group].reduce((total, status) => total + (counts[status] ?? 0), 0);
}
export const rejectReasons = ['already_available', 'not_available', 'duplicate', 'out_of_scope', 'storage', 'other'] as const;
export const rejectReasonLabels: Readonly<Record<string, string>> = translatedLabels({ already_available: 'reject_reason_already_available', not_available: 'reject_reason_not_available', duplicate: 'reject_reason_duplicate', out_of_scope: 'reject_reason_out_of_scope', storage: 'reject_reason_storage', other: 'reject_reason_other' });
export const priorityLabels: Readonly<Record<Priority, string>> = translatedLabels({ high: 'priority_high', normal: 'priority_normal', low: 'priority_low' });
export const checkLabels: Readonly<Record<string, string>> = translatedLabels({ file_presence: 'check_file_presence', library_scan: 'check_library_scan', video_decode: 'check_video_decode', playback_engine: 'check_playback_engine', audio_track: 'check_audio_track', audio_default_track: 'check_audio_default_track', subtitle_track: 'check_subtitle_track', subtitle_selection: 'check_subtitle_selection', subtitle_timing: 'check_subtitle_timing', metadata_match: 'check_metadata_match', timestamp: 'check_timestamp' });
export const historyLabels: Readonly<Record<string, string>> = translatedLabels({ 'report.created': 'history_report_created', 'report.comment': 'history_report_comment', 'report.status': 'history_report_status', 'report.priority': 'history_report_priority', 'report.imported': 'history_report_imported' });
/** `1:02:03` / `4:05` for a playback position in seconds. */
export function formatPosition(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '';
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600), minutes = Math.floor(total % 3600 / 60), rest = total % 60;
  const pad = (value: number) => String(value).padStart(2, '0');
  return hours ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`;
}
/** Season numbers grouped as ranges: `1–3, 5`. */
export function seasonRanges(numbers: number[]): string {
  const sorted = [...new Set(numbers)].sort((a, b) => a - b);
  const parts: string[] = [];
  for (let index = 0; index < sorted.length; index++) {
    const start = sorted[index];
    while (index + 1 < sorted.length && sorted[index + 1] === sorted[index] + 1) index++;
    parts.push(start === sorted[index] ? String(start) : `${start}–${sorted[index]}`);
  }
  return parts.join(', ');
}
/** Catalogue page of a work, when the kind has one. */
export function workHref(kind: string | undefined, workId: string): `films/?work=${string}` | `series/?work=${string}` | null {
  if (kind === 'film') return `films/?work=${encodeURIComponent(workId)}`;
  if (kind === 'series') return `series/?work=${encodeURIComponent(workId)}`;
  return null;
}
/** Strict admission body accepted by `POST /requests`: discovery metadata never enters it. */
export type RequestPayload = Pick<SearchResult, 'kind' | 'title' | 'externalId' | 'externalProvider' | 'serverId'> & Required<Omit<RequestSelection, 'serverId' | 'qualityProfileId'>> & { qualityProfileId?: number };
export function requestPayload(result: SearchResult, selection: RequestSelection): RequestPayload {
  return { kind: result.kind, title: result.title, externalId: result.externalId, externalProvider: result.externalProvider,
    ...(selection.serverId || result.serverId ? { serverId: selection.serverId || result.serverId } : {}),
    ...(selection.qualityProfileId ? { qualityProfileId: selection.qualityProfileId } : {}),
    seasons: selection.seasons ?? [], episodes: selection.episodes ?? [], albumIds: selection.albumIds ?? [], monitorFuture: selection.monitorFuture ?? false };
}
export function dateLabel(value: number | string | undefined): string {
  if (!value) return '—';
  const date = new Date(typeof value === 'number' ? value * 1000 : value);
  return Number.isNaN(date.valueOf()) ? '—' : currentFormat().dateTime(date);
}
export function parseSelection(seasons: string, episodes: string): { seasons: number[]; episodes: { season: number; episode: number }[] } {
  if (seasons.trim() && seasons.split(',').some(item => !item.trim())) throw new Error(tr('enter_season_numbers_separated_by_commas'));
  const selectedSeasons = seasons.trim() ? seasons.split(',').map(item => Number(item.trim())) : [];
  if (selectedSeasons.some(value => !Number.isInteger(value) || value < 0 || value > 1000)) throw new Error(tr('enter_season_numbers_separated_by_commas'));
  const selectedEpisodes = episodes.trim() ? episodes.split(',').map(item => {
    const match = /^\s*(\d{1,4})\s*[:x]\s*(\d{1,4})\s*$/.exec(item);
    if (!match || Number(match[2]) < 1) throw new Error(tr('enter_episodes_in_the_form_1'));
    return { season: Number(match[1]), episode: Number(match[2]) };
  }) : [];
  return { seasons: [...new Set(selectedSeasons)], episodes: selectedEpisodes.filter((value, index, all) => all.findIndex(other => other.season === value.season && other.episode === value.episode) === index) };
}
export function reportPayload(input: ReportInput): ReportInput {
  if (!input.description.trim()) throw new Error(tr('describe_the_problem_you_ran_into'));
  if (!input.workId && !input.trackId) throw new Error(tr('select_a_media_item_from_its'));
  for (const value of [input.positionSeconds, input.endSeconds]) if (value !== undefined && (!Number.isFinite(value) || value < 0)) throw new Error(tr('the_position_must_be_a_positive'));
  if (input.endSeconds !== undefined && (input.positionSeconds === undefined || input.endSeconds < input.positionSeconds)) throw new Error(tr('the_end_must_come_after_the'));
  const context = input.context ? Object.fromEntries(['engine', 'errorCode', 'audioTrackId', 'subtitleTrackId'].filter(key => typeof input.context?.[key as keyof ReportContext] === 'string').map(key => [key, input.context?.[key as keyof ReportContext]])) : undefined;
  return { category: input.category, description: input.description.trim(), workId: input.workId, trackId: input.trackId, assetId: input.assetId, positionSeconds: input.positionSeconds, endSeconds: input.endSeconds, context };
}

export interface QuotaRule { limit: number; days: number }
export interface ClassRule { allow?: boolean; autoApprove?: boolean; quota?: QuotaRule }
export interface PolicyRule { film?: ClassRule; series?: ClassRule; music?: ClassRule }
export interface EffectiveQuota { limit: number; days: number; used: number; remaining: number; resetsAt?: number | null }
export interface EffectiveClassPolicy { allow: boolean; autoApprove: boolean; destination?: boolean; quota: EffectiveQuota | null }
export interface MyPolicy {
  administrator: boolean;
  pending: number;
  maxPending: number;
  restricted: boolean;
  classes: { film: EffectiveClassPolicy; series: EffectiveClassPolicy; music: EffectiveClassPolicy };
}
export interface PolicyGroup { id: string; name: string; isDefault: boolean; systemRole?: 'user' | 'administrator' | null; rule: PolicyRule | null }
export interface PolicyUser { id: string; label?: string; rule: PolicyRule }
export interface PolicyOverview { default: PolicyRule; autoApprove: boolean; maxPending: number; groups: PolicyGroup[]; users: PolicyUser[] }

/** Safe proxied URL for title artwork not yet present in the local library. */
export function posterUrl(poster: { src: string; path: string } | null | undefined): string {
  if (!poster?.src || !poster?.path) return '';
  return `/api/v1/requests/poster?src=${encodeURIComponent(poster.src)}&path=${encodeURIComponent(poster.path)}`;
}

/** Combined discovery search across configured servers with rights and library annotations. */
export async function discoverRequests(q: string, signal?: AbortSignal): Promise<{ results: SearchResult[]; errors: { error: string }[]; restricted?: boolean }> {
  return applicationRequest<{ results: SearchResult[]; errors: { error: string }[]; restricted?: boolean }>(
    `/api/v1/requests/discover?q=${encodeURIComponent(q.trim())}`,
    { signal }
  );
}

export async function loadMyPolicy(): Promise<MyPolicy> {
  return applicationRequest<MyPolicy>('/api/v1/requests/policy');
}
export async function loadPolicyOverview(): Promise<PolicyOverview> {
  return applicationRequest<PolicyOverview>('/api/v1/admin/acquisition/policies');
}
export async function saveDefaultPolicy(rule: PolicyRule): Promise<PolicyRule> {
  return applicationRequest<PolicyRule>('/api/v1/admin/acquisition/policies/default', {
    method: 'PUT',
    body: JSON.stringify(rule)
  });
}
export async function saveGroupPolicy(groupId: string, rule: PolicyRule): Promise<PolicyRule> {
  return applicationRequest<PolicyRule>(`/api/v1/admin/acquisition/policies/groups/${encodeURIComponent(groupId)}`, {
    method: 'PUT',
    body: JSON.stringify(rule)
  });
}
export async function deleteGroupPolicy(groupId: string): Promise<void> {
  return applicationRequest<void>(`/api/v1/admin/acquisition/policies/groups/${encodeURIComponent(groupId)}`, {
    method: 'DELETE'
  });
}
export async function saveUserPolicy(userId: string, rule: PolicyRule): Promise<PolicyRule> {
  return applicationRequest<PolicyRule>(`/api/v1/admin/acquisition/policies/users/${encodeURIComponent(userId)}`, {
    method: 'PUT',
    body: JSON.stringify(rule)
  });
}
export async function deleteUserPolicy(userId: string): Promise<void> {
  return applicationRequest<void>(`/api/v1/admin/acquisition/policies/users/${encodeURIComponent(userId)}`, {
    method: 'DELETE'
  });
}
export async function loadUserEffectivePolicy(userId: string): Promise<{ rule: PolicyRule | null; effective: MyPolicy }> {
  return applicationRequest<{ rule: PolicyRule | null; effective: MyPolicy }>(
    `/api/v1/admin/acquisition/policies/users/${encodeURIComponent(userId)}/effective`
  );
}

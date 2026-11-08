import type { TranslationKey } from '../i18n';

/** Shapes of `/api/v1/admin/tasks/automation/*` and the installed extensions it refers to. */
export type AutomationMode = 'automatic' | 'scheduled' | 'manual' | 'disabled';
export const automationModes: readonly AutomationMode[] = ['automatic', 'scheduled', 'manual', 'disabled'];
export interface AutomationPolicy {
  extensionId: string; libraryId: string; mode: AutomationMode; priority: number; operation: string;
  configRevision: string; algorithmVersion: string; intervalSeconds: number; nextRunAt: number; maxConcurrent: number;
}
export interface AutomationExtension {
  manifest: { id: string; name: string; permissions: string[]; httpHosts?: string[] };
  enabled: boolean; libraryIds?: string[]; allLibraries?: boolean;
}
export interface AutomationRun {
  id: string; extensionId: string; libraryId: string; rootLibraryId?: string; libraryName?: string;
  status: string; operation: string; provider?: string; priority: number; attempt: number; maxAttempts?: number;
  target?: string; createdAt?: number; startedAt?: number | null; finishedAt?: number | null;
  durationSeconds: number | null; error: string;
}
export interface ProviderQuota { provider: string; nextAllowedAt: number; minIntervalSeconds: number; maxConcurrent: number }
export interface MetadataSource { workId: string; workTitle?: string; field: string; extensionId: string; providerId: string; priority: number; updatedAt: number }
export interface LibraryRef { id: string; name: string }

export const DEFAULT_PRIORITY = 100;
export const DEFAULT_INTERVAL = 86400;
/** Intervals offered for scheduled rules, in seconds; a saved custom value stays selectable. */
export function intervalChoices(current: number): number[] {
  const choices = [6 * 3600, 12 * 3600, 86400, 2 * 86400, 7 * 86400, 30 * 86400];
  return choices.includes(current) ? choices : [...choices, current].sort((a, b) => a - b);
}

/** The rule that applies: the saved policy, or the server default (automatic, priority 100). */
export interface EffectiveRule { mode: AutomationMode; priority: number; intervalSeconds: number; nextRunAt: number; saved: boolean }
export function effectiveRule(policies: AutomationPolicy[], extensionId: string, libraryId: string): EffectiveRule {
  const policy = policies.find(p => p.extensionId === extensionId && p.libraryId === libraryId);
  return policy
    ? { mode: policy.mode, priority: policy.priority, intervalSeconds: policy.intervalSeconds, nextRunAt: policy.nextRunAt, saved: true }
    : { mode: 'automatic', priority: DEFAULT_PRIORITY, intervalSeconds: DEFAULT_INTERVAL, nextRunAt: 0, saved: false };
}

/** Libraries the extension is allowed to read (all of them, or its grants). */
export function accessibleLibraries<L extends LibraryRef>(extension: AutomationExtension, libraries: L[]): L[] {
  if (extension.allLibraries ?? true) return libraries;
  const granted = new Set(extension.libraryIds ?? []);
  return libraries.filter(library => granted.has(library.id));
}

/** Libraries grouped by the mode that applies to them, in the display order of the modes. */
export function ruleSummary(policies: AutomationPolicy[], extensionId: string, libraries: LibraryRef[]): { mode: AutomationMode; names: string[] }[] {
  return automationModes
    .map(mode => ({ mode, names: libraries.filter(library => effectiveRule(policies, extensionId, library.id).mode === mode).map(library => library.name) }))
    .filter(group => group.names.length);
}

/** The policy to save for an edited rule. Unchanged schedules keep their next run. */
export function policyFor(
  previous: AutomationPolicy | undefined, extension: AutomationExtension, libraryId: string,
  draft: { mode: AutomationMode; priority: number; intervalSeconds: number }
): AutomationPolicy {
  const network = !!extension.manifest.httpHosts?.length;
  const sameSchedule = previous?.mode === 'scheduled' && draft.mode === 'scheduled' && previous.intervalSeconds === draft.intervalSeconds;
  return {
    extensionId: extension.manifest.id, libraryId, mode: draft.mode,
    priority: Math.min(1000, Math.max(0, Math.round(draft.priority))),
    operation: previous?.operation ?? (network ? 'metadata.search' : 'metadata.propose'),
    configRevision: previous?.configRevision ?? '', algorithmVersion: previous?.algorithmVersion ?? '1',
    intervalSeconds: Math.min(31536000, Math.max(60, Math.round(draft.intervalSeconds))),
    nextRunAt: sameSchedule ? previous.nextRunAt : 0,
    maxConcurrent: previous?.maxConcurrent ?? 1
  };
}

export type RunGroup = 'active' | 'ok' | 'failed' | 'cancelled';
export function runGroup(status: string): RunGroup {
  if (status === 'queued' || status === 'running' || status === 'paused') return 'active';
  if (status === 'completed' || status === 'succeeded') return 'ok';
  if (status === 'error' || status === 'failed') return 'failed';
  return 'cancelled';
}
export function runCounts(runs: AutomationRun[]): Record<RunGroup | 'all', number> {
  const counts = { all: runs.length, active: 0, ok: 0, failed: 0, cancelled: 0 };
  for (const run of runs) counts[runGroup(run.status)] += 1;
  return counts;
}

export const runStatusKeys: Record<string, TranslationKey> = {
  queued: 'automation_status_queued', running: 'automation_status_running', paused: 'automation_status_paused',
  completed: 'automation_status_completed', succeeded: 'automation_status_completed', error: 'automation_status_failed',
  failed: 'automation_status_failed', cancelled: 'automation_status_cancelled', interrupted: 'automation_status_cancelled'
};

/** Task errors are machine codes, optionally followed by the extension's own code (`extension_error:provider_error`). */
const errorKeys: Record<string, TranslationKey> = {
  extension_timeout: 'automation_error_timeout', playback_preempted: 'automation_error_preempted',
  automation_cancelled: 'automation_error_cancelled', metadata_unidentified: 'automation_error_unidentified',
  provider_error: 'automation_error_provider', not_found: 'automation_error_not_found', rate_limited: 'automation_error_rate_limited',
  missing_api_key: 'automation_error_api_key', invalid_api_key: 'automation_error_api_key', unauthorized: 'automation_error_api_key',
  network_error: 'automation_error_network', parse_error: 'automation_error_parse', invalid_output: 'automation_error_parse'
};
export function errorKey(code: string): TranslationKey {
  const [host, plugin] = code.split(':', 2);
  return errorKeys[plugin ?? ''] ?? errorKeys[host] ?? 'automation_error_generic';
}
/** Missing or rejected keys are fixed in the extension settings, not by retrying. */
export function needsSettings(code: string): boolean {
  return errorKey(code) === 'automation_error_api_key';
}

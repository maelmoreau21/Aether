/** Shapes and helpers of the `/api/v1/admin/logs` API (files, level, live lines). */
export type LogLevel = 'debug' | 'info' | 'warning' | 'error';
export const logLevels: readonly LogLevel[] = ['debug', 'info', 'warning', 'error'];
export const logLevelRank: Record<LogLevel, number> = { debug: 0, info: 1, warning: 2, error: 3 };

export interface LogSettings { level: LogLevel; retentionDays: number; maxRetentionDays: number; directory: string | null; filesEnabled: boolean }
export interface LogFile { name: string; size: number; modifiedAt: number }
export interface LogLine { index: number; stamp: string; level: LogLevel; message: string }

/** Retention choices offered; a custom value already saved stays selectable. */
export function retentionChoices(current: number): number[] {
  const choices = [3, 7, 14, 30, 90, 365];
  return choices.includes(current) ? choices : [...choices, current].sort((a, b) => a - b);
}

/** `aether-core-2026-11-08.log` → `2026-11-08` (UTC day the file covers). */
export function logFileDay(name: string): string | null {
  return /^aether-core-(\d{4}-\d{2}-\d{2})\.log$/.exec(name)?.[1] ?? null;
}

export function fileSizeLabel(bytes: number, locale = 'fr'): string {
  const units = ['o', 'Ko', 'Mo', 'Go'];
  const english = ['B', 'KB', 'MB', 'GB'];
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: digits }).format(value);
  return `${number} ${(locale.startsWith('fr') ? units : english)[unit]}`;
}

const tags: Record<string, LogLevel> = { DEBUG: 'debug', INFO: 'info', WARN: 'warning', ERROR: 'error' };

/** Parses the lines the Core writes: `<RFC 3339> <LEVEL> <message>`; anything else continues as info. */
export function parseLogText(text: string): LogLine[] {
  const lines: LogLine[] = [];
  text.split('\n').forEach((raw, index) => {
    if (!raw.trim()) return;
    const match = /^(\d{4}-\d{2}-\d{2}T[\d:.]+Z) (DEBUG|INFO|WARN|ERROR) +(.*)$/.exec(raw);
    lines.push(match
      ? { index, stamp: match[1], level: tags[match[2]], message: match[3] }
      : { index, stamp: '', level: 'info', message: raw });
  });
  return lines;
}

// Locale-aware formatting. Every date, number and comparison shown to a user goes through here so
// that the interface language, not the browser or the machine locale, decides the output.
import type { SupportedLocale } from './messages';

/** BCP 47 tag used for Intl. The interface offers two languages; regions are the conventional ones. */
export const INTL_TAG: Record<SupportedLocale, string> = { fr: 'fr-FR', en: 'en-GB' };

export function intlTag(locale: SupportedLocale): string {
  return INTL_TAG[locale] ?? INTL_TAG.fr;
}

const dateFormats = new Map<string, Intl.DateTimeFormat>();
const numberFormats = new Map<string, Intl.NumberFormat>();
const collators = new Map<string, Intl.Collator>();

function dateFormat(locale: SupportedLocale, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = dateFormats.get(key);
  if (!format) { format = new Intl.DateTimeFormat(intlTag(locale), options); dateFormats.set(key, format); }
  return format;
}

function numberFormat(locale: SupportedLocale, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = numberFormats.get(key);
  if (!format) { format = new Intl.NumberFormat(intlTag(locale), options); numberFormats.set(key, format); }
  return format;
}

/** Unix seconds (the API convention), milliseconds-based Date, or an ISO string. */
export type DateInput = number | Date | string;

/** The API sends Unix seconds; `Date` and ISO strings pass through. */
export function toDate(value: DateInput): Date {
  return value instanceof Date ? value : typeof value === 'number' ? new Date(value * 1000) : new Date(value);
}

const invalid = '—';

export function formatDate(locale: SupportedLocale, value: DateInput | null | undefined, style: 'short' | 'medium' | 'long' = 'medium'): string {
  if (value === null || value === undefined) return invalid;
  const date = toDate(value);
  return Number.isNaN(date.getTime()) ? invalid : dateFormat(locale, { dateStyle: style }).format(date);
}

export function formatDateTime(locale: SupportedLocale, value: DateInput | null | undefined, style: 'short' | 'medium' = 'medium'): string {
  if (value === null || value === undefined) return invalid;
  const date = toDate(value);
  return Number.isNaN(date.getTime()) ? invalid : dateFormat(locale, { dateStyle: style, timeStyle: 'short' }).format(date);
}

export function formatTime(locale: SupportedLocale, value: DateInput | null | undefined): string {
  if (value === null || value === undefined) return invalid;
  const date = toDate(value);
  return Number.isNaN(date.getTime()) ? invalid : dateFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(date);
}

export function formatNumber(locale: SupportedLocale, value: number | null | undefined, options: Intl.NumberFormatOptions = {}): string {
  return value === null || value === undefined || !Number.isFinite(value) ? invalid : numberFormat(locale, options).format(value);
}

export function formatPercent(locale: SupportedLocale, ratio: number | null | undefined, fractionDigits = 0): string {
  return formatNumber(locale, ratio, { style: 'percent', maximumFractionDigits: fractionDigits });
}

/** A decimal with a fixed number of digits, for ratings (`7,5` / `7.5`). */
export function formatDecimal(locale: SupportedLocale, value: number | null | undefined, digits = 1): string {
  return formatNumber(locale, value, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** Locale-aware, accent- and case-insensitive, natural ("Episode 2" before "Episode 10") comparison. */
export function compareText(locale: SupportedLocale, a: string, b: string): number {
  let collator = collators.get(locale);
  if (!collator) {
    collator = new Intl.Collator(intlTag(locale), { numeric: true, sensitivity: 'base' });
    collators.set(locale, collator);
  }
  return collator.compare(a, b);
}

/** Language names in the interface language (`fr` → "français" / "French"). */
export function languageName(locale: SupportedLocale, code: string): string {
  try {
    return new Intl.DisplayNames([intlTag(locale)], { type: 'language' }).of(code) ?? code;
  } catch {
    return code;
  }
}

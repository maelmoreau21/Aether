// Pure helpers for the statistics charts and the Wrapped story. No DOM access.
import type { Wrapped } from './api';

/** IANA zone of the browser, else minutes east of UTC (both accepted by the Core). */
export function browserTimeZone(): string {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (zone && /^[A-Za-z0-9/_+-]{1,64}$/.test(zone)) return zone;
  } catch { /* Older engines: fall back to the current offset. */ }
  return String(-new Date().getTimezoneOffset());
}

/** Local calendar date (YYYY-MM-DD) of a Date. */
export function localDate(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Unix seconds of the local midnight starting a YYYY-MM-DD date. */
export function localDayStart(value: string): number {
  const [year, month, day] = value.split('-').map(Number);
  return Math.floor(new Date(year, month - 1, day).getTime() / 1000);
}

/** Shift a YYYY-MM-DD date by whole days (calendar arithmetic, DST-proof). */
export function addDays(value: string, days: number): string {
  const [year, month, day] = value.split('-').map(Number);
  const shifted = new Date(Date.UTC(year, month - 1, day + days));
  return shifted.toISOString().slice(0, 10);
}

export type Direction = 'up' | 'down' | 'flat' | 'none';
/** Growth percentage (null when there is nothing to compare with). */
export function growthInfo(value: number | null | undefined, locale = 'fr-FR'): { direction: Direction; label: string } {
  if (value === null || value === undefined || !Number.isFinite(value)) return { direction: 'none', label: '' };
  if (Math.abs(value) < 0.5) return { direction: 'flat', label: '=' };
  const rounded = new Intl.NumberFormat(locale, { maximumFractionDigits: Math.abs(value) < 10 ? 1 : 0 }).format(Math.abs(value));
  return { direction: value > 0 ? 'up' : 'down', label: `${value > 0 ? '+' : '−'}${rounded} %` };
}

/** Intensity level 0–4 for heatmaps. */
export function level(value: number, max: number): number {
  if (value <= 0 || max <= 0) return 0;
  const ratio = value / max;
  return ratio > 0.75 ? 4 : ratio > 0.5 ? 3 : ratio > 0.25 ? 2 : 1;
}

/** 7 rows (Monday first) × 24 hours from the Core's 168 cells. */
export function hoursMatrix(cells: { cell: number; plays: number; seconds: number }[], metric: 'plays' | 'seconds' = 'plays'): number[][] {
  const rows = Array.from({ length: 7 }, () => Array<number>(24).fill(0));
  for (const cell of cells) {
    if (cell.cell >= 0 && cell.cell < 168) rows[Math.floor(cell.cell / 24)][cell.cell % 24] = cell[metric];
  }
  return rows;
}

export interface CalendarCell { date: string; seconds: number; plays: number; level: number; inRange: boolean }
/** Week columns (Monday → Sunday) covering `from`…`to`, at most the last 53 weeks. */
export function calendarWeeks(days: { date: string; plays: number; seconds: number }[], from: string, to: string): CalendarCell[][] {
  const values = new Map(days.map(day => [day.date, day]));
  const max = Math.max(0, ...days.map(day => day.seconds));
  const limit = addDays(to, -370);
  const first = from < limit ? limit : from;
  const weekday = (value: string) => (new Date(`${value}T00:00:00Z`).getUTCDay() + 6) % 7;
  let cursor = addDays(first, -weekday(first));
  const end = addDays(to, 6 - weekday(to));
  const weeks: CalendarCell[][] = [];
  while (cursor <= end) {
    const week: CalendarCell[] = [];
    for (let i = 0; i < 7; i++) {
      const day = values.get(cursor);
      week.push({ date: cursor, seconds: day?.seconds ?? 0, plays: day?.plays ?? 0, level: level(day?.seconds ?? 0, max), inRange: cursor >= first && cursor <= to });
      cursor = addDays(cursor, 1);
    }
    weeks.push(week);
  }
  return weeks;
}

/** Compact duration: "45 min", "3 h 05", "128 h". */
export function shortDuration(seconds: number): string {
  const minutes = Math.round(Math.max(0, seconds) / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60), rest = minutes % 60;
  return hours >= 100 || rest === 0 ? `${hours} h` : `${hours} h ${String(rest).padStart(2, '0')}`;
}

/** Localised weekday (0 = Monday) and month (1–12) names. */
export function weekdayName(index: number, locale: string, style: 'long' | 'short' = 'long'): string {
  return new Intl.DateTimeFormat(locale, { weekday: style, timeZone: 'UTC' }).format(new Date(Date.UTC(2024, 0, 1 + index)));
}
export function monthName(month: number, locale: string, style: 'long' | 'short' = 'long'): string {
  return new Intl.DateTimeFormat(locale, { month: style, timeZone: 'UTC' }).format(new Date(Date.UTC(2024, month - 1, 1)));
}

export type SlideId = 'intro' | 'time' | 'monthly' | 'rhythm' | 'streak' | 'films' | 'series' | 'binge' | 'music' | 'genres' | 'people' | 'device' | 'moments' | 'compare' | 'summary' | 'empty';
/** Story slides that have something true to show, in order. */
export function wrappedSlides(w: Pick<Wrapped, 'totals' | 'films' | 'series' | 'binge' | 'artists' | 'genres' | 'people' | 'topDevice' | 'streak' | 'first' | 'previous' | 'peaks'>): SlideId[] {
  if (!w.totals.plays) return ['empty'];
  const slides: SlideId[] = ['intro', 'time', 'monthly'];
  if (w.peaks.hour !== null || w.peaks.weekday !== null) slides.push('rhythm');
  if (w.streak.days >= 2) slides.push('streak');
  if (w.films.length) slides.push('films');
  if (w.series.length) slides.push('series');
  if (w.binge) slides.push('binge');
  if (w.artists.length) slides.push('music');
  if (w.genres.length) slides.push('genres');
  if (w.people.length) slides.push('people');
  if (w.topDevice) slides.push('device');
  if (w.first) slides.push('moments');
  if (w.previous) slides.push('compare');
  slides.push('summary');
  return slides;
}

export interface SummaryLabels {
  title: string; hours: string; plays: string; titles: string; topFilm: string; topSeries: string; topArtist: string; topGenre: string; streak: string; footer: string;
}
/** Plain-text recap for the "copy" button. Labels arrive translated, with {placeholders}. */
export function wrappedSummary(w: Wrapped, labels: SummaryLabels, locale = 'fr-FR'): string {
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
  const fill = (text: string, values: Record<string, string | number>) => Object.entries(values).reduce((result, [key, value]) => result.replaceAll(`{${key}}`, String(value)), text);
  const lines = [
    fill(labels.title, { year: w.year, name: w.user.name }),
    fill(labels.hours, { hours: number.format(w.totals.hours) }),
    fill(labels.plays, { plays: number.format(w.totals.plays) }),
    fill(labels.titles, { titles: number.format(w.totals.titles) })
  ];
  if (w.films[0]) lines.push(fill(labels.topFilm, { title: w.films[0].title }));
  if (w.series[0]) lines.push(fill(labels.topSeries, { title: w.series[0].title }));
  if (w.artists[0]) lines.push(fill(labels.topArtist, { title: w.artists[0].title }));
  if (w.genres[0]) lines.push(fill(labels.topGenre, { title: w.genres[0].title }));
  if (w.streak.days >= 2) lines.push(fill(labels.streak, { days: w.streak.days }));
  lines.push(labels.footer);
  return lines.join('\n');
}

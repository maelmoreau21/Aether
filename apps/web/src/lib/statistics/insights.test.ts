import { describe, expect, it } from 'vitest';
import type { Wrapped } from './api';
import { addDays, calendarWeeks, growthInfo, hoursMatrix, level, localDate, localDayStart, shortDuration, wrappedSlides, wrappedSummary } from './insights';
import { translate } from './i18n';

const rank = (id: string, title: string) => ({ id, title, subtitle: '', image: '', kind: 'film', seconds: 3600, plays: 2, users: 1, items: 1 });
function wrapped(overrides: Partial<Wrapped> = {}): Wrapped {
  return {
    visible: true, settings: { visible: true, periodEnabled: false, startMonth: 12, startDay: 1, endMonth: 1, endDay: 31 },
    year: 2025, kind: 'all', timezone: 'Europe/Paris', availableYears: [2026, 2025], user: { id: 'u', name: 'Maël' }, generatedAt: 0,
    totals: { seconds: 360000, hours: 100, plays: 120, titles: 40, days: 4.2, activeDays: 90 },
    monthly: [], hours: Array(24).fill(0), weekdays: Array(7).fill(0), peaks: { hour: 21, weekday: 5, month: 12 },
    streak: { days: 6, start: '2025-12-20', end: '2025-12-25' }, topDevice: 'Firefox · Windows', devices: [],
    genres: [rank('drame', 'Drame')], films: [rank('f', 'Le film')], series: [], binge: null,
    artists: [], albums: [], tracks: [], people: [], kinds: { film: { plays: 1, seconds: 1 }, episode: { plays: 0, seconds: 0 }, music: { plays: 0, seconds: 0 } },
    first: { title: 'Le film', kind: 'film', at: 0, image: '' }, last: null, previous: null,
    ...overrides
  };
}

describe('statistics helpers', () => {
  it('round-trips local calendar dates', () => {
    expect(localDate(new Date(localDayStart('2026-03-29') * 1000))).toBe('2026-03-29');
    expect(addDays('2024-02-28', 1)).toBe('2024-02-29');
    expect(addDays('2026-01-01', -1)).toBe('2025-12-31');
  });

  it('formats growth and intensity levels', () => {
    expect(growthInfo(null).direction).toBe('none');
    expect(growthInfo(0.2).direction).toBe('flat');
    expect(growthInfo(25, 'en-GB')).toEqual({ direction: 'up', label: '+25 %' });
    expect(growthInfo(-3.25, 'en-GB')).toEqual({ direction: 'down', label: '−3.3 %' });
    expect([0, 1, 30, 60, 100].map(value => level(value, 100))).toEqual([0, 1, 2, 3, 4]);
  });

  it('maps the 168 hour cells to weekday rows', () => {
    const rows = hoursMatrix([{ cell: 0, plays: 2, seconds: 10 }, { cell: 24 * 6 + 23, plays: 5, seconds: 50 }]);
    expect(rows).toHaveLength(7);
    expect(rows[0][0]).toBe(2);
    expect(rows[6][23]).toBe(5);
  });

  it('lays the calendar out in Monday-first weeks', () => {
    // 2026-03-04 is a Wednesday, 2026-03-10 a Tuesday.
    const weeks = calendarWeeks([{ date: '2026-03-05', plays: 1, seconds: 100 }], '2026-03-04', '2026-03-10');
    expect(weeks).toHaveLength(2);
    expect(weeks[0][0].date).toBe('2026-03-02');
    expect(weeks[0][0].inRange).toBe(false);
    expect(weeks[0][3]).toMatchObject({ date: '2026-03-05', level: 4, inRange: true });
    expect(weeks[1][6].date).toBe('2026-03-15');
    expect(calendarWeeks([], '2020-01-01', '2026-03-10').length).toBeLessThanOrEqual(54);
  });

  it('formats compact durations', () => {
    expect(shortDuration(45 * 60)).toBe('45 min');
    expect(shortDuration(3 * 3600 + 5 * 60)).toBe('3 h 05');
    expect(shortDuration(128 * 3600 + 120)).toBe('128 h');
  });

  it('only shows Wrapped slides backed by data', () => {
    expect(wrappedSlides(wrapped({ totals: { seconds: 0, hours: 0, plays: 0, titles: 0, days: 0, activeDays: 0 } }))).toEqual(['empty']);
    const slides = wrappedSlides(wrapped());
    expect(slides[0]).toBe('intro');
    expect(slides.at(-1)).toBe('summary');
    expect(slides).toContain('films');
    expect(slides).toContain('streak');
    expect(slides).not.toContain('series');
    expect(slides).not.toContain('music');
    expect(slides).not.toContain('compare');
  });

  it('builds a translated plain-text summary', () => {
    const labels = (language: string) => ({
      title: translate(language, 's_title'), hours: translate(language, 's_hours'), plays: translate(language, 's_plays'), titles: translate(language, 's_titles'),
      topFilm: translate(language, 's_film'), topSeries: translate(language, 's_series'), topArtist: translate(language, 's_artist'),
      topGenre: translate(language, 's_genre'), streak: translate(language, 's_streak'), footer: translate(language, 's_footer')
    });
    const french = wrappedSummary(wrapped(), labels('fr'), 'fr-FR');
    expect(french).toContain('Mon Wrapped 2025 · Maël');
    expect(french).toContain('100 heures de lecture');
    expect(french).toContain('Film n° 1 : Le film');
    expect(french).toContain('Série record : 6 jours');
    expect(french).not.toContain('Série n° 1');
    const english = wrappedSummary(wrapped(), labels('en'), 'en-GB');
    expect(english.split('\n')[0]).toBe('My 2025 Wrapped · Maël');
    expect(english).toContain('Favourite genre: Drame');
  });
});

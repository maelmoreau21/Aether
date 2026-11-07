import type { Work } from './api';

import { tr, currentFormat } from '../i18n';
export type DiscoverSort = 'added-desc' | 'added-asc' | 'year-desc' | 'year-asc' | 'title-asc';
export type DiscoverSpec = { key: string; title: string; kind: 'film' | 'series'; sort: DiscoverSort; genre?: string };

function hash(value: string): number {
  let result = 2166136261;
  for (let index = 0; index < value.length; index++) {
    result ^= value.charCodeAt(index);
    result = Math.imul(result, 16777619);
  }
  return result >>> 0;
}

/** Mélange déterministe pour une graine donnée (une visite = un ordre stable). */
export function shuffleWithSeed<T>(items: T[], seed: string): T[] {
  const result = [...items];
  let state = hash(seed) || 1;
  for (let index = result.length - 1; index > 0; index--) {
    state = Math.imul(state ^ (state >>> 15), 2246822507) >>> 0;
    state = Math.imul(state ^ (state >>> 13), 3266489909) >>> 0;
    const target = state % (index + 1);
    [result[index], result[target]] = [result[target], result[index]];
  }
  return result;
}

function topGenres(works: Work[], seed: string, limit: number): string[] {
  const counts = new Map<string, { label: string; count: number }>();
  for (const work of works) {
    for (const genre of work.genres ?? []) {
      const label = genre.trim();
      if (!label || label.length > 100) continue;
      const key = label.toLowerCase();
      const entry = counts.get(key);
      if (entry) entry.count++;
      else counts.set(key, { label, count: 1 });
    }
  }
  const ranked = [...counts.values()].sort((a, b) => b.count - a.count || currentFormat().compare(a.label, b.label));
  // Les genres les plus fréquents restent en tête, mais l'ordre varie d'une visite à l'autre.
  const head = shuffleWithSeed(ranked.slice(0, 4), `${seed}:head`);
  const tail = shuffleWithSeed(ranked.slice(4), `${seed}:tail`);
  return [...head, ...tail].slice(0, limit).map(entry => entry.label);
}

/**
 * Plan des rangées du fil de l'accueil : genres présents dans la médiathèque,
 * films et séries en alternance, entrecoupés de rangées « récentes » et « à redécouvrir ».
 */
export function buildDiscoverPlan(works: Work[], seed: string): DiscoverSpec[] {
  const films = works.filter(work => work.kind !== 'series');
  const series = works.filter(work => work.kind === 'series');
  const sorts: DiscoverSort[] = ['added-desc', 'year-desc', 'title-asc'];
  const genreSpec = (kind: 'film' | 'series', genre: string, index: number): DiscoverSpec => ({
    key: `${kind}:genre:${genre.toLowerCase()}`,
    title: `${kind === 'film' ? tr('films') : tr('series')} · ${genre}`,
    kind,
    genre,
    sort: sorts[(hash(`${seed}:${kind}:${genre}`) + index) % sorts.length]
  });
  const filmGenres = films.length ? topGenres(films, `${seed}:film`, 12).map((genre, index) => genreSpec('film', genre, index)) : [];
  const seriesGenres = series.length ? topGenres(series, `${seed}:series`, 12).map((genre, index) => genreSpec('series', genre, index)) : [];
  const extras: DiscoverSpec[] = [
    ...(films.length ? [
      { key: 'film:rediscover', title: tr('films_to_rediscover'), kind: 'film', sort: 'added-asc' },
      { key: 'film:recent-release', title: tr('the_most_recent_films'), kind: 'film', sort: 'year-desc' },
      { key: 'film:classics', title: tr('the_classics_of_your_library'), kind: 'film', sort: 'year-asc' }
    ] satisfies DiscoverSpec[] : []),
    ...(series.length ? [
      { key: 'series:rediscover', title: tr('series_to_rediscover'), kind: 'series', sort: 'added-asc' },
      { key: 'series:recent-release', title: tr('the_most_recent_series'), kind: 'series', sort: 'year-desc' }
    ] satisfies DiscoverSpec[] : [])
  ];
  const genres: DiscoverSpec[] = [];
  for (let index = 0; index < Math.max(filmGenres.length, seriesGenres.length); index++) {
    if (filmGenres[index]) genres.push(filmGenres[index]);
    if (seriesGenres[index]) genres.push(seriesGenres[index]);
  }
  const shuffledExtras = shuffleWithSeed(extras, `${seed}:extras`);
  const plan: DiscoverSpec[] = [];
  // Une rangée spéciale toutes les trois rangées de genre.
  for (let index = 0; index < genres.length || shuffledExtras.length; index++) {
    if (index < genres.length) plan.push(genres[index]);
    if ((index % 3 === 2 || index >= genres.length) && shuffledExtras.length) plan.push(shuffledExtras.shift()!);
    if (index >= genres.length && !shuffledExtras.length) break;
  }
  return plan;
}

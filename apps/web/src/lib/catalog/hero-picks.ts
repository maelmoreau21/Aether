import type { Work } from './api';

/** Nombre d'œuvres à l'affiche (grande image + colonne de vignettes). */
export const HERO_COUNT = 4;
/** Profondeur de tirage dans chaque liste (les plus récentes d'abord). */
export const HERO_SCAN = 48;

/** Graine aléatoire d'une visite de l'accueil. */
export function newHeroSeed(): number {
  return Math.floor(Math.random() * 0x1_0000_0000);
}

// FNV-1a 32 bits : ordre aléatoire mais stable pour une graine donnée,
// pour que l'affiche ne se remélange pas à chaque mise à jour réactive des listes.
function rank(id: string, seed: number): number {
  let hash = (0x811c9dc5 ^ seed) >>> 0;
  for (let index = 0; index < id.length; index++) {
    hash ^= id.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

export function shuffleWithSeed<T extends { id: string }>(items: T[], seed: number): T[] {
  return items
    .map(item => ({ item, key: rank(item.id, seed) }))
    .sort((a, b) => a.key - b.key)
    .map(entry => entry.item);
}

/**
 * Tire au hasard (selon `seed`) des œuvres avec une grande image, films et séries en alternance ;
 * une liste trop courte est complétée par l'autre.
 */
export function pickHero(films: Work[], series: Work[], hasWideArt: (work: Work) => boolean, seed: number, count = HERO_COUNT): Work[] {
  const pools = [films, series].map((pool, index) => shuffleWithSeed(pool.slice(0, HERO_SCAN).filter(hasWideArt), seed + index));
  if (seed % 2) pools.reverse();
  const picked: Work[] = [];
  const seen = new Set<string>();
  for (let index = 0; picked.length < count && index < HERO_SCAN; index++) {
    for (const pool of pools) {
      const work = pool[index];
      if (work && picked.length < count && !seen.has(work.id)) {
        seen.add(work.id);
        picked.push(work);
      }
    }
  }
  return picked;
}

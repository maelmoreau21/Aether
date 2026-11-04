import type { Work } from './api';

/** Forme d'une tuile de la mosaïque « Pour ce soir » (toutes occupent deux rangées). */
export type TileShape = 'wide' | 'tall' | 'square';
export type TileBreakpoint = 'desktop' | 'tablet' | 'phone';
export type MosaicTile = { shape: TileShape; span: Record<TileBreakpoint, number>; hidden: Record<TileBreakpoint, boolean> };

export const TONIGHT_MIN = 4;
export const TONIGHT_MAX = 7;
const RECENT_WINDOW = 24;

const COLUMNS: Record<TileBreakpoint, number> = { desktop: 6, tablet: 4, phone: 2 };
const SPANS: Record<TileBreakpoint, Record<TileShape, number>> = {
  desktop: { wide: 3, tall: 1, square: 2 },
  tablet: { wide: 4, tall: 1, square: 2 },
  phone: { wide: 2, tall: 1, square: 1 }
};
// Suites choisies pour remplir exactement les bandes de 6 colonnes sur PC.
const PATTERNS: Record<number, TileShape[]> = {
  4: ['wide', 'tall', 'square', 'wide'],
  5: ['wide', 'tall', 'square', 'wide', 'wide'],
  6: ['wide', 'tall', 'square', 'square', 'tall', 'wide'],
  7: ['wide', 'tall', 'square', 'tall', 'square', 'square', 'tall']
};

function interleave(first: Work[], second: Work[]): Work[] {
  const merged: Work[] = [];
  for (let index = 0; index < Math.max(first.length, second.length); index++) {
    if (first[index]) merged.push(first[index]);
    if (second[index]) merged.push(second[index]);
  }
  return merged;
}

/**
 * Ajouts récents (films et séries mêlés), hors œuvres déjà à l'affiche quand c'est possible,
 * en privilégiant celles qui ont une image. Moins de quatre œuvres : aucune mosaïque.
 */
export function pickTonight(films: Work[], series: Work[], excludedIds: string[], hasArtwork: (work: Work) => boolean, limit = TONIGHT_MAX): Work[] {
  const seen = new Set<string>();
  // Tri stable : sans date d'ajout, l'ordre déjà « récent d'abord » des listes est conservé.
  const recent = interleave(films, series)
    .filter(work => !seen.has(work.id) && Boolean(seen.add(work.id)))
    .sort((a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0))
    .slice(0, RECENT_WINDOW);
  const excluded = new Set(excludedIds);
  const fresh = recent.filter(work => !excluded.has(work.id));
  const ranked = [...fresh.filter(hasArtwork), ...fresh.filter(work => !hasArtwork(work))];
  const picked = ranked.slice(0, limit);
  // Pas assez d'œuvres nouvelles : on complète avec celles de l'affiche.
  for (const work of recent) {
    if (picked.length >= limit) break;
    if (excluded.has(work.id)) picked.push(work);
  }
  return picked.length >= TONIGHT_MIN ? picked : [];
}

/** Répartit les tuiles par bandes de deux rangées ; une bande incomplète est comblée par sa dernière tuile. */
export function mosaicLayout(count: number): MosaicTile[] {
  const size = Math.min(TONIGHT_MAX, count);
  if (size < TONIGHT_MIN) return [];
  const tiles: MosaicTile[] = PATTERNS[size].map(shape => ({
    shape,
    span: { desktop: 0, tablet: 0, phone: 0 },
    hidden: { desktop: false, tablet: false, phone: false }
  }));
  for (const breakpoint of Object.keys(COLUMNS) as TileBreakpoint[]) {
    const columns = COLUMNS[breakpoint];
    let band: MosaicTile[] = [];
    let used = 0;
    const close = () => {
      const last = band.at(-1);
      if (!last || used >= columns) return;
      // Une affiche seule étirée sur toute la largeur serait trop recadrée : on la masque.
      if (band.length === 1 && last.shape !== 'wide' && tiles.filter(tile => !tile.hidden[breakpoint]).length > TONIGHT_MIN) last.hidden[breakpoint] = true;
      else last.span[breakpoint] += columns - used;
    };
    for (const tile of tiles) {
      const span = Math.min(SPANS[breakpoint][tile.shape], columns);
      if (used + span > columns) {
        close();
        band = [];
        used = 0;
      }
      tile.span[breakpoint] = span;
      band.push(tile);
      used += span;
    }
    close();
  }
  return tiles;
}

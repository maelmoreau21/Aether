import type { Work } from './api';

/**
 * Mosaïque « Pour ce soir » : une tuile vedette 16:9 (image paysage) suivie d'affiches 2:3.
 * Aucun autre format : la grille est dimensionnée par `aspect-ratio`, jamais par une hauteur fixe.
 */
export type TileShape = 'feature' | 'poster';
export type TileBreakpoint = 'desktop' | 'tablet' | 'phone';
export type MosaicTile = { shape: TileShape; hidden: Record<TileBreakpoint, boolean> };
/**
 * Grille d'un point de rupture. Ordinateur : vedette à gauche sur `featureRows` rangées
 * d'affiches. Tablette et téléphone : vedette pleine largeur, affiches dessous.
 */
export type MosaicGrid = { columns: number; featureRows: number };
export type MosaicLayout = { posters: number; tiles: MosaicTile[]; grids: Record<TileBreakpoint, MosaicGrid> };

export const TONIGHT_MIN = 4;
export const TONIGHT_MAX = 7;
const RECENT_WINDOW = 24;

// Par nombre d'affiches : chaque grille est remplie exactement, sans case vide.
const GRIDS: Record<number, Record<TileBreakpoint, MosaicGrid>> = {
  3: { desktop: { columns: 3, featureRows: 1 }, tablet: { columns: 3, featureRows: 0 }, phone: { columns: 3, featureRows: 0 } },
  4: { desktop: { columns: 2, featureRows: 2 }, tablet: { columns: 4, featureRows: 0 }, phone: { columns: 2, featureRows: 0 } },
  // Cinq affiches ne remplissent pas deux colonnes : la dernière est masquée sur téléphone.
  5: { desktop: { columns: 5, featureRows: 1 }, tablet: { columns: 5, featureRows: 0 }, phone: { columns: 2, featureRows: 0 } },
  6: { desktop: { columns: 3, featureRows: 2 }, tablet: { columns: 6, featureRows: 0 }, phone: { columns: 3, featureRows: 0 } }
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

/** Place en vedette la première œuvre qui a une image paysage ; l'ordre des autres est conservé. */
export function arrangeTonight(works: Work[], hasWideArtwork: (work: Work) => boolean): Work[] {
  const index = works.findIndex(hasWideArtwork);
  return index <= 0 ? works : [works[index], ...works.slice(0, index), ...works.slice(index + 1)];
}

/** Une vedette puis des affiches ; les affiches qui ne complètent pas une rangée sont masquées. */
export function mosaicLayout(count: number): MosaicLayout | null {
  const size = Math.min(TONIGHT_MAX, count);
  if (size < TONIGHT_MIN) return null;
  const posters = size - 1;
  const grids = GRIDS[posters];
  const tiles: MosaicTile[] = Array.from({ length: size }, (_, index) => {
    const hidden = { desktop: false, tablet: false, phone: false };
    if (index > 0) {
      for (const breakpoint of Object.keys(grids) as TileBreakpoint[]) {
        const { columns } = grids[breakpoint];
        hidden[breakpoint] = index > posters - (posters % columns);
      }
    }
    return { shape: index === 0 ? 'feature' : 'poster', hidden };
  });
  return { posters, tiles, grids };
}

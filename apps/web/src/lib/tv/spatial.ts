import type { Direction } from './keys';

export type Box = { left: number; top: number; right: number; bottom: number };

const EDGE_TOLERANCE = 2;
/** Weight of the sideways drift: rows and columns win over the nearest diagonal. */
const DRIFT_WEIGHT = 2;
const CENTRE_WEIGHT = 0.1;

/**
 * Index of the box to reach from `from` in `direction`, or -1. A candidate must start past the current
 * box's far edge (a few pixels of overlap allowed); the score is the gap along the axis plus the weighted
 * sideways drift, which is zero when both boxes share a row (or a column). So Right goes to the next card of
 * the same rail, Down goes to the most aligned card of the next rail, and the grid order follows the layout.
 */
export function nearestBox(from: Box, candidates: readonly Box[], direction: Direction): number {
  let best = -1;
  let bestScore = Infinity;
  candidates.forEach((box, index) => {
    const score = boxScore(from, box, direction);
    if (score < bestScore) { bestScore = score; best = index; }
  });
  return best;
}

function boxScore(from: Box, box: Box, direction: Direction): number {
  const horizontal = direction === 'left' || direction === 'right';
  const gap = direction === 'right' ? box.left - from.right
    : direction === 'left' ? from.left - box.right
    : direction === 'down' ? box.top - from.bottom
    : from.top - box.bottom;
  if (gap < -EDGE_TOLERANCE) return Infinity;
  // The candidate's centre must also move the right way: a taller box overlapping the edge does not count.
  const centre = horizontal ? (box.left + box.right) / 2 - (from.left + from.right) / 2 : (box.top + box.bottom) / 2 - (from.top + from.bottom) / 2;
  if ((direction === 'right' || direction === 'down') ? centre <= 0 : centre >= 0) return Infinity;
  const [start, end, otherStart, otherEnd] = horizontal ? [from.top, from.bottom, box.top, box.bottom] : [from.left, from.right, box.left, box.right];
  const drift = otherEnd < start ? start - otherEnd : otherStart > end ? otherStart - end : 0;
  // Between equally aligned boxes, the one centred closest wins (a card touching the edge is not aligned).
  const offCentre = Math.abs((otherStart + otherEnd) / 2 - (start + end) / 2);
  return Math.max(0, gap) + drift * DRIFT_WEIGHT + offCentre * CENTRE_WEIGHT;
}

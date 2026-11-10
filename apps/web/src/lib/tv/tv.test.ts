import { describe, expect, it } from 'vitest';
import { isDirection, tvKey } from './keys';
import { nearestBox, type Box } from './spatial';

const box = (left: number, top: number, width = 100, height = 60): Box => ({ left, top, right: left + width, bottom: top + height });

describe('TV remote keys', () => {
  it('reads named keys and the Tizen and webOS key codes', () => {
    expect(tvKey({ key: 'ArrowLeft', keyCode: 37 })).toBe('left');
    expect(tvKey({ key: 'Unidentified', keyCode: 10009 })).toBe('back');
    expect(tvKey({ key: 'XF86Back', keyCode: 10009 })).toBe('back');
    expect(tvKey({ key: 'GoBack', keyCode: 461 })).toBe('back');
    expect(tvKey({ key: 'Unidentified', keyCode: 461 })).toBe('back');
    expect(tvKey({ key: 'Escape', keyCode: 27 })).toBe('back');
    expect(tvKey({ key: 'MediaPlayPause', keyCode: 10252 })).toBe('playPause');
    expect(tvKey({ key: 'Unidentified', keyCode: 415 })).toBe('play');
    expect(tvKey({ key: 'Unidentified', keyCode: 19 })).toBe('pause');
    expect(tvKey({ key: 'Unidentified', keyCode: 417 })).toBe('forward');
    expect(tvKey({ key: 'Unidentified', keyCode: 412 })).toBe('rewind');
    expect(tvKey({ key: 'MediaStop', keyCode: 413 })).toBe('stop');
  });

  it('never turns a typed character into a remote key', () => {
    // "&" shares key code 38 with ArrowUp on some layouts and "(" 40 with ArrowDown.
    expect(tvKey({ key: '&', keyCode: 38 })).toBeNull();
    expect(tvKey({ key: '(', keyCode: 40 })).toBeNull();
    expect(tvKey({ key: 'a', keyCode: 65 })).toBeNull();
    expect(isDirection(tvKey({ key: 'ArrowDown', keyCode: 40 }))).toBe(true);
    expect(isDirection(tvKey({ key: 'Enter', keyCode: 13 }))).toBe(false);
  });
});

describe('spatial navigation', () => {
  // Two rails of four cards under a navigation bar with two links.
  const nav = [box(40, 0, 80, 30), box(140, 0, 80, 30)];
  const rail1 = [0, 1, 2, 3].map(i => box(40 + i * 120, 100));
  const rail2 = [0, 1, 2, 3].map(i => box(40 + i * 120, 220));
  const all = [...nav, ...rail1, ...rail2];

  it('moves along a rail and stops at its end', () => {
    expect(nearestBox(rail1[1], all, 'right')).toBe(all.indexOf(rail1[2]));
    expect(nearestBox(rail1[1], all, 'left')).toBe(all.indexOf(rail1[0]));
    expect(nearestBox(rail1[3], all, 'right')).toBe(-1);
  });

  it('goes to the aligned card of the next rail, not the nearest diagonal', () => {
    expect(nearestBox(rail1[2], all, 'down')).toBe(all.indexOf(rail2[2]));
    expect(nearestBox(rail2[3], all, 'up')).toBe(all.indexOf(rail1[3]));
    expect(nearestBox(rail2[0], all, 'down')).toBe(-1);
  });

  it('reaches the navigation bar from the first rail', () => {
    expect(nearestBox(rail1[0], all, 'up')).toBe(all.indexOf(nav[0]));
    expect(nearestBox(nav[1], all, 'down')).toBe(all.indexOf(rail1[1]));
  });

  it('follows a grid row by row, even when the last row is shorter', () => {
    const grid = [box(0, 0), box(120, 0), box(240, 0), box(0, 100), box(120, 100)];
    expect(nearestBox(grid[2], grid, 'down')).toBe(4);
    expect(nearestBox(grid[4], grid, 'up')).toBe(1);
    expect(nearestBox(grid[3], grid, 'right')).toBe(4);
  });

  it('ignores a box overlapping the current one', () => {
    const wide = box(0, 0, 400, 60);
    const inside = box(150, 10, 40, 40);
    expect(nearestBox(wide, [inside], 'right')).toBe(-1);
  });
});

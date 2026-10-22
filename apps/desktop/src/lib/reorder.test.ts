import { describe, expect, it } from 'vitest';

import { dropAfter, gapAt } from './reorder';

describe('reorder', () => {
  const rows = [
    { top: 0, bottom: 40 },
    { top: 40, bottom: 80 },
    { top: 80, bottom: 120 },
  ];

  it('finds the gap nearest the pointer', () => {
    expect(gapAt(rows, -10)).toBe(0);
    expect(gapAt(rows, 19)).toBe(0);
    expect(gapAt(rows, 21)).toBe(1);
    expect(gapAt(rows, 100)).toBe(3);
    expect(gapAt([], 5)).toBe(0);
  });

  it('names the entry a dropped row goes after', () => {
    // Queue positions in play order.
    const order = [7, 3, 9, 4];

    expect(dropAfter(order, 2, 0)).toBeNull();
    expect(dropAfter(order, 0, 3)).toBe(9);
    expect(dropAfter(order, 0, 4)).toBe(4);
    // Its own place, either side: no move.
    expect(dropAfter(order, 1, 1)).toBeUndefined();
    expect(dropAfter(order, 1, 2)).toBeUndefined();
  });
});

/**
 * Drag to reorder a list. Gaps are numbered from 0 (above the first row) to
 * the row count (below the last).
 */

/** Pointer travel before a press becomes a drag, in pixels. */
export const DRAG_THRESHOLD = 4;

/** The gap nearest `y`, given each row's top and bottom edge. */
export function gapAt(
  rows: readonly { top: number; bottom: number }[],
  y: number,
) {
  const below = rows.findIndex((row) => y < (row.top + row.bottom) / 2);

  return below === -1 ? rows.length : below;
}

/**
 * Where row `from` lands when dropped in `gap`: the entry it goes after
 * (`null`: first), or `undefined` when it does not move.
 */
export function dropAfter<T>(order: readonly T[], from: number, gap: number) {
  if (gap === from || gap === from + 1) return undefined;

  return gap === 0 ? null : (order[gap - 1] ?? undefined);
}

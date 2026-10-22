/**
 * Track rows, as in desktop music players: a click selects, a double click
 * or Enter plays. Spread on the row's button.
 */
export function rowEvents(select: () => void, play: () => void) {
  return {
    onclick: select,
    ondblclick: play,
    onkeydown: (event: KeyboardEvent) => {
      if (event.key !== 'Enter') return;

      // Also stops Enter from clicking the button (which only selects).
      event.preventDefault();
      play();
    },
  };
}

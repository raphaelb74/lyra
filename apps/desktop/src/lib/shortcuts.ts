/** Keyboard shortcuts (docs/DESIGN_SPEC.md §8). Volume is Ctrl+↑/↓ in VolumeControl. */

export type Action =
  | 'toggle'
  | 'next'
  | 'previous'
  | 'seekForward'
  | 'seekBack'
  | 'stop'
  | 'search'
  | 'albums'
  | 'artists'
  | 'tracks'
  | 'playlists'
  | 'queue'
  | 'settings'
  /** Browser keys a music player should not have (reload, print). */
  | 'ignore';

export interface Keys {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

const VIEWS: readonly Action[] = [
  'albums',
  'artists',
  'tracks',
  'playlists',
  'queue',
];

function mediaAction(key: string): Action | null {
  switch (key) {
    case 'MediaPlayPause':
      return 'toggle';
    case 'MediaTrackNext':
      return 'next';
    case 'MediaTrackPrevious':
      return 'previous';
    case 'MediaStop':
      return 'stop';
    default:
      return null;
  }
}

/** What a key press does, or null to leave it to the focused control. */
export function actionFor(keys: Keys): Action | null {
  const { key, ctrlKey, shiftKey, altKey, metaKey } = keys;

  // Media keys reach the window when the desktop does not take them for
  // MPRIS (src-tauri/src/mpris.rs).
  const media = mediaAction(key);

  if (media !== null) return media;

  if (altKey || metaKey) return null;

  if (ctrlKey) {
    if (key === 'ArrowRight') return 'next';

    if (key === 'ArrowLeft') return 'previous';

    if (key === '.') return 'stop';

    if (key === ',') return 'settings';

    if (key === 'f' || key === 'F') return 'search';

    if (key === 'r' || key === 'R' || key === 'p' || key === 'P') {
      return 'ignore';
    }

    const view = /^[1-5]$/.test(key) ? VIEWS[Number(key) - 1] : undefined;

    return view ?? null;
  }

  if (shiftKey) {
    if (key === 'ArrowRight') return 'seekForward';

    if (key === 'ArrowLeft') return 'seekBack';

    return null;
  }

  if (key === ' ') return 'toggle';

  if (key === '/') return 'search';

  if (key === 'F5') return 'ignore';

  return null;
}

/** A shortcut as Settings lists it. */
export interface Shortcut {
  label: string;
  /** What `actionFor` does with each press; null when a control handles it. */
  action: Action | null;
  presses: readonly Keys[];
}

function keys(key: string, modifiers: Partial<Keys> = {}): Keys {
  return {
    key,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...modifiers,
  };
}

const ctrl = { ctrlKey: true };

export const SHORTCUTS: readonly Shortcut[] = [
  { label: 'Play or pause', action: 'toggle', presses: [keys(' ')] },
  {
    label: 'Next track',
    action: 'next',
    presses: [keys('ArrowRight', ctrl)],
  },
  {
    label: 'Previous track',
    action: 'previous',
    presses: [keys('ArrowLeft', ctrl)],
  },
  {
    label: 'Forward 10 seconds',
    action: 'seekForward',
    presses: [keys('ArrowRight', { shiftKey: true })],
  },
  {
    label: 'Back 10 seconds',
    action: 'seekBack',
    presses: [keys('ArrowLeft', { shiftKey: true })],
  },
  { label: 'Stop', action: 'stop', presses: [keys('.', ctrl)] },
  {
    label: 'Volume',
    action: null,
    presses: [keys('ArrowUp', ctrl), keys('ArrowDown', ctrl)],
  },
  {
    label: 'Search tracks',
    action: 'search',
    presses: [keys('/'), keys('f', ctrl)],
  },
  { label: 'Albums', action: 'albums', presses: [keys('1', ctrl)] },
  { label: 'Artists', action: 'artists', presses: [keys('2', ctrl)] },
  { label: 'Tracks', action: 'tracks', presses: [keys('3', ctrl)] },
  { label: 'Playlists', action: 'playlists', presses: [keys('4', ctrl)] },
  {
    label: 'Show or hide the queue',
    action: 'queue',
    presses: [keys('5', ctrl)],
  },
  { label: 'Settings', action: 'settings', presses: [keys(',', ctrl)] },
  {
    label: 'Move in the queue',
    action: null,
    presses: [
      keys('ArrowUp', { altKey: true }),
      keys('ArrowDown', { altKey: true }),
    ],
  },
  { label: 'Close or go back', action: null, presses: [keys('Escape')] },
];

const KEY_NAMES = new Map([
  [' ', 'Space'],
  ['ArrowRight', '→'],
  ['ArrowLeft', '←'],
  ['ArrowUp', '↑'],
  ['ArrowDown', '↓'],
  ['Escape', 'Esc'],
]);

function modifierNames(press: Keys): string[] {
  const names: string[] = [];

  if (press.ctrlKey) names.push('Ctrl');

  if (press.shiftKey) names.push('Shift');

  if (press.altKey) names.push('Alt');

  return names;
}

function keyName(press: Keys): string {
  return KEY_NAMES.get(press.key) ?? press.key.toUpperCase();
}

/** The keys of one press as printed on the keyboard: `['Ctrl', 'F']`. */
export function keyNames(press: Keys): string[] {
  return [...modifierNames(press), keyName(press)];
}

/** Presses with the same modifiers, shown once: `Ctrl + ↑ or ↓`. */
export interface KeyGroup {
  modifiers: string[];
  keys: string[];
}

export function keyGroups(presses: readonly Keys[]): KeyGroup[] {
  const groups: KeyGroup[] = [];

  for (const press of presses) {
    const modifiers = modifierNames(press);
    const last = groups.at(-1);

    if (last?.modifiers.join('+') === modifiers.join('+')) {
      last.keys.push(keyName(press));
    } else {
      groups.push({ modifiers, keys: [keyName(press)] });
    }
  }

  return groups;
}

/** Text fields keep their keys: typing a space or a slash is not a shortcut. */
export function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  if (target.isContentEditable) return true;

  if (target instanceof HTMLTextAreaElement) return true;

  if (target instanceof HTMLSelectElement) return true;

  return (
    target instanceof HTMLInputElement &&
    !['range', 'radio', 'checkbox', 'button'].includes(target.type)
  );
}

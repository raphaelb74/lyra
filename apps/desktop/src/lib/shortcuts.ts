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

/** What a key press does, or null to leave it to the focused control. */
export function actionFor(keys: Keys): Action | null {
  const { key, ctrlKey, shiftKey, altKey, metaKey } = keys;

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

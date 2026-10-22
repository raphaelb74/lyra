import { describe, expect, it } from 'vitest';

import {
  actionFor,
  keyGroups,
  keyNames,
  SHORTCUTS,
  type Action,
  type Keys,
} from './shortcuts';

function press(key: string, modifiers: Partial<Keys> = {}): Keys {
  return {
    key,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...modifiers,
  };
}

describe('keyboard shortcuts', () => {
  it('play and pause with Space, search with /', () => {
    expect(actionFor(press(' '))).toBe('toggle');
    expect(actionFor(press('/'))).toBe('search');
    expect(actionFor(press('f', { ctrlKey: true }))).toBe('search');
  });

  it('move through the queue and the track', () => {
    expect(actionFor(press('ArrowRight', { ctrlKey: true }))).toBe('next');
    expect(actionFor(press('ArrowLeft', { ctrlKey: true }))).toBe('previous');
    expect(actionFor(press('ArrowRight', { shiftKey: true }))).toBe(
      'seekForward',
    );
    expect(actionFor(press('ArrowLeft', { shiftKey: true }))).toBe('seekBack');
    expect(actionFor(press('.', { ctrlKey: true }))).toBe('stop');
  });

  it('open views with Ctrl and a number, Settings with Ctrl+,', () => {
    expect(actionFor(press('1', { ctrlKey: true }))).toBe('albums');
    expect(actionFor(press('5', { ctrlKey: true }))).toBe('queue');
    expect(actionFor(press('6', { ctrlKey: true }))).toBeNull();
    expect(actionFor(press(',', { ctrlKey: true }))).toBe('settings');
  });

  it('obey the keyboard media keys', () => {
    expect(actionFor(press('MediaPlayPause'))).toBe('toggle');
    expect(actionFor(press('MediaTrackNext'))).toBe('next');
    expect(actionFor(press('MediaTrackPrevious'))).toBe('previous');
    expect(actionFor(press('MediaStop'))).toBe('stop');
  });

  it('swallow browser reload and print', () => {
    expect(actionFor(press('F5'))).toBe('ignore');
    expect(actionFor(press('r', { ctrlKey: true }))).toBe('ignore');
    expect(actionFor(press('P', { ctrlKey: true, shiftKey: true }))).toBe(
      'ignore',
    );
  });

  it('leave plain keys and other combinations alone', () => {
    expect(actionFor(press('a'))).toBeNull();
    expect(actionFor(press('ArrowDown'))).toBeNull();
    expect(actionFor(press(' ', { altKey: true }))).toBeNull();
  });
});

describe('the shortcut list in Settings', () => {
  it('lists presses that do what their label says', () => {
    for (const { label, action, presses } of SHORTCUTS) {
      for (const keys of presses) {
        expect(actionFor(keys), `${label}: ${keyNames(keys).join('+')}`).toBe(
          action,
        );
      }
    }
  });

  it('lists every action', () => {
    // A new action fails type checking here until it is added.
    const every: Record<Exclude<Action, 'ignore'>, true> = {
      toggle: true,
      next: true,
      previous: true,
      seekForward: true,
      seekBack: true,
      stop: true,
      search: true,
      albums: true,
      artists: true,
      tracks: true,
      playlists: true,
      queue: true,
      settings: true,
    };

    const listed = new Set(SHORTCUTS.map((shortcut) => shortcut.action));

    for (const action of Object.keys(every)) {
      expect(listed, action).toContain(action);
    }
  });

  it('names keys as printed on the keyboard', () => {
    expect(keyNames(press(' '))).toEqual(['Space']);
    expect(keyNames(press('f', { ctrlKey: true }))).toEqual(['Ctrl', 'F']);
    expect(keyNames(press('ArrowUp', { altKey: true }))).toEqual(['Alt', '↑']);
  });

  it('prints shared modifiers once', () => {
    const ctrl = { ctrlKey: true };

    expect(
      keyGroups([press('ArrowUp', ctrl), press('ArrowDown', ctrl)]),
    ).toEqual([{ modifiers: ['Ctrl'], keys: ['↑', '↓'] }]);
    expect(keyGroups([press('/'), press('f', ctrl)])).toEqual([
      { modifiers: [], keys: ['/'] },
      { modifiers: ['Ctrl'], keys: ['F'] },
    ]);
  });
});

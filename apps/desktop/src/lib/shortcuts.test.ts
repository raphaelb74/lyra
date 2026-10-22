import { describe, expect, it } from 'vitest';

import { actionFor, type Keys } from './shortcuts';

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

/**
 * Lyra's own right-click menu: one at a time, drawn by ContextMenu.svelte.
 * The webview's menu (Reload, Inspect) is turned off outside text fields.
 */

import type { IconName } from '../assets/icons/lucide';
import { attempt, type Backend } from './ipc';

export type MenuItem =
  | {
      label: string;
      icon?: IconName;
      danger?: boolean;
      disabled?: boolean;
      /** Leaves the menu open, e.g. to show a list in its place. */
      keep?: boolean;
      run: () => void;
    }
  /** A text field with its placeholder, e.g. a new playlist's name. */
  | { field: string; submit: (text: string) => void }
  | 'separator';

interface OpenMenu {
  x: number;
  y: number;
  items: MenuItem[];
}

export const menu = $state<{ open: OpenMenu | null }>({ open: null });

/**
 * Opens `items` where the pointer is; from the keyboard (Menu key,
 * Shift+F10), under the focused element.
 */
export function openMenu(event: MouseEvent, items: MenuItem[]) {
  event.preventDefault();
  event.stopPropagation();

  let { clientX: x, clientY: y } = event;

  if (x === 0 && y === 0 && event.currentTarget instanceof Element) {
    const box = event.currentTarget.getBoundingClientRect();

    x = box.left + box.width / 2;
    y = box.bottom;
  }

  menu.open = { x, y, items };
}

export function closeMenu() {
  menu.open = null;
}

/** Shows other items in the open menu, in its place. */
export function replaceItems(items: MenuItem[]) {
  if (menu.open !== null) menu.open = { ...menu.open, items };
}

/** "Play next" and "Add to queue"; when nothing plays, they play the tracks. */
export function queueActions(
  api: Backend,
  /** The tracks, loaded only when an action is chosen. */
  tracks: () => Promise<number[]>,
  device: string,
  onError: (message: string) => void,
): MenuItem[] {
  async function enqueue(next: boolean) {
    const ids = await attempt(tracks, onError);

    if (ids !== undefined && ids.length > 0) {
      await attempt(() => api.enqueue(ids, next, device), onError);
    }
  }

  const disabled = device === '';

  return [
    {
      label: 'Play next',
      icon: 'list-start',
      disabled,
      run: () => void enqueue(true),
    },
    {
      label: 'Add to queue',
      icon: 'list-end',
      disabled,
      run: () => void enqueue(false),
    },
  ];
}

/** "Add to playlist…": lists the playlists in the menu, and a field for a new one. */
export function addToPlaylist(
  api: Backend,
  /** The tracks to add, loaded only when a playlist is chosen. */
  tracks: () => Promise<number[]>,
  onError: (message: string) => void,
): MenuItem {
  async function add(id: number) {
    const ids = await attempt(tracks, onError);

    if (ids !== undefined && ids.length > 0) {
      await attempt(() => api.addToPlaylist(id, ids), onError);
    }
  }

  async function show() {
    const playlists = (await attempt(api.playlists, onError)) ?? [];

    replaceItems([
      ...playlists.map((playlist): MenuItem => ({
        label: playlist.name,
        icon: 'list-music',
        run: () => void add(playlist.id),
      })),
      ...(playlists.length > 0 ? ['separator' as const] : []),
      {
        field: 'New playlist',
        submit: (name: string) => {
          void attempt(() => api.createPlaylist(name), onError).then((id) => {
            if (id !== undefined) void add(id);
          });
        },
      },
    ]);
  }

  return {
    label: 'Add to playlist…',
    icon: 'plus',
    keep: true,
    run: () => void show(),
  };
}

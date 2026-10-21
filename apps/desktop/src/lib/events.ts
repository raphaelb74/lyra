import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import {
  isRecord,
  isText,
  isDroppedFile,
  isVolume,
  isScanStatus,
  isStatus,
} from './guards';
import type {
  DroppedFile,
  JsonValue,
  PlayerStatus,
  ScanStatus,
  VolumeInfo,
} from './ipc';

/* Events Rust pushes to the window. */

/** Status pushed by Rust after every playback state change. */
export function onStatus(
  handle: (status: PlayerStatus) => void,
): Promise<UnlistenFn> {
  return listen<JsonValue>('player-status', (event) => {
    if (isStatus(event.payload)) handle(event.payload);
  });
}

/** Playable files the user dropped onto the window (possibly none). */
export function onFilesDropped(
  handle: (files: DroppedFile[]) => void,
): Promise<UnlistenFn> {
  return listen<JsonValue>('files-dropped', (event) => {
    const files = event.payload;

    if (Array.isArray(files) && files.every(isDroppedFile)) handle(files);
  });
}

/** Scan progress and results. */
export function onScanStatus(
  handle: (status: ScanStatus) => void,
): Promise<UnlistenFn> {
  return listen<JsonValue>('scan-status', (event) => {
    if (isScanStatus(event.payload)) handle(event.payload);
  });
}

/** A device's volume changed, in Lyra or elsewhere (volume keys, PipeWire). */
export function onDeviceVolume(
  handle: (device: string, volume: VolumeInfo | null) => void,
): Promise<UnlistenFn> {
  return listen<JsonValue>('device-volume', (event) => {
    const payload = event.payload;

    if (
      isRecord(payload) &&
      isText(payload.device) &&
      isVolume(payload.volume)
    ) {
      handle(payload.device, payload.volume);
    }
  });
}

/** Folders or albums changed: reload them. */
export function onLibraryChanged(handle: () => void): Promise<UnlistenFn> {
  return listen('library-changed', () => {
    handle();
  });
}

/** A sentence for the user about a folder they added (e.g. already covered). */
export function onLibraryNotice(
  handle: (message: string) => void,
): Promise<UnlistenFn> {
  return listen<JsonValue>('library-notice', (event) => {
    if (isText(event.payload)) handle(event.payload);
  });
}

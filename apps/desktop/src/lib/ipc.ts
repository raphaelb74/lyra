import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/*
 * Typed boundary to the Rust adapter (apps/desktop/src-tauri/src/ipc.rs).
 * Every response is validated before use; nothing is trusted by shape alone.
 */

/** Mirrors Rust's `AppInfo`. */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}

/** Mirrors Rust's `DeviceInfo`. */
export interface DeviceInfo {
  /** ALSA name, `hw:CARD=…,DEV=…`. */
  name: string;
  cardName: string;
  pcmName: string;
  usb: boolean;
}

export interface DroppedFile {
  path: string;
  name: string;
}

export type PlaybackState =
  | 'stopped'
  | 'loading'
  | 'negotiating'
  | 'buffering'
  | 'playing'
  | 'paused'
  | 'error';

export interface Verdict {
  status: 'yes' | 'no' | 'unverified';
  reason: string | null;
}

export interface Output {
  transport: 'pcm' | 'dop' | 'native';
  format: string;
  sampleRate: number;
  channels: number;
}

export interface Track {
  /** Library track id; null for a dropped file. */
  libraryId: number | null;
  /** Position in the queue. */
  queueIndex: number;
  fileName: string;
  device: string;
  codec: 'flac' | 'wav' | 'dsf' | 'dff';
  /** PCM sample rate, or DSD bit rate per channel. */
  sampleRate: number;
  /** 1 for DSD. */
  bitsPerSample: number;
  /** 64 for DSD64; null for PCM. */
  dsdMultiple: number | null;
  channels: number;
  durationSeconds: number | null;
  output: Output | null;
  bitPerfect: Verdict | null;
}

/** Mirrors Rust's `FolderInfo`: a folder the library scans. */
export interface Folder {
  id: number;
  path: string;
}

/** Source format shared by albums and tracks. */
export interface SourceFormat {
  codec: string;
  /** PCM sample rate, or DSD bit rate per channel. */
  sampleRate: number;
  dsdMultiple: number | null;
}

/** Mirrors Rust's `AlbumInfo`; the format is the best among its tracks. */
export interface Album extends SourceFormat {
  id: number;
  title: string;
  artist: string;
  year: number | null;
  /** `lyra-art://` URL of the cover, or null. */
  art: string | null;
  trackCount: number;
}

/** Mirrors Rust's `LibraryTrack`. */
export interface LibraryTrack extends SourceFormat {
  id: number;
  title: string;
  artist: string;
  album: string;
  albumId: number;
  disc: number | null;
  track: number | null;
  durationSeconds: number | null;
  /** 1 for DSD. */
  bitsPerSample: number;
  channels: number;
}

/** Mirrors Rust's `ArtistInfo`: an album artist. */
export interface Artist {
  id: number;
  name: string;
  albumCount: number;
  trackCount: number;
}

/** Mirrors Rust's `PlaylistInfo`. */
export interface Playlist {
  id: number;
  name: string;
  trackCount: number;
  durationSeconds: number;
}

/** Mirrors Rust's `PlaylistEntry`; a track may appear twice. */
export interface PlaylistEntry {
  entryId: number;
  track: LibraryTrack;
}

export interface ScanReport {
  added: number;
  updated: number;
  removed: number;
  failed: number;
}

/** Mirrors Rust's `ScanStatus`. */
export interface ScanStatus {
  running: boolean;
  done: number;
  total: number;
  last: ScanReport | null;
  error: string | null;
}

/** Mirrors Rust's `Status`. */
export interface PlayerStatus {
  state: PlaybackState;
  track: Track | null;
  error: { code: string; message: string } | null;
  queueLength: number;
}

/** Mirrors Rust's `QueueEntry`. */
export interface QueueEntry {
  trackId: number | null;
  title: string;
  artist: string | null;
  durationSeconds: number | null;
}

/** Mirrors Rust's `TrackDetails`: library data for Now Playing. */
export interface TrackDetails {
  title: string;
  artist: string;
  album: string | null;
  albumId: number;
  year: number | null;
  art: string | null;
}

/** Errors raised in the UI itself, plus the `code`s Rust sends. */
export type BackendErrorCode =
  'browser_preview' | 'command_failed' | 'invalid_response' | (string & {});

export class BackendError extends Error {
  readonly code: BackendErrorCode;

  constructor(code: BackendErrorCode, message: string, cause?: unknown) {
    super(message, { cause });
    this.name = 'BackendError';
    this.code = code;
  }
}

/** Anything Tauri IPC can deliver: commands return serialized JSON. */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };

export type CommandArgs = Record<string, JsonValue>;

/** Sends one Tauri command; injectable so tests need no module mocking. */
export type RunCommand = (
  command: string,
  args?: CommandArgs,
) => Promise<JsonValue>;

type Guard<T> = (value: JsonValue) => value is JsonValue & T;

/** A field read from a record may be absent. */
type Field = JsonValue | undefined;

function isRecord(value: Field): value is Readonly<Record<string, JsonValue>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isText(value: Field): value is string {
  return typeof value === 'string';
}

function isNumber(value: Field): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isFlag(value: Field): value is boolean {
  return typeof value === 'boolean';
}

function isNullableText(value: Field): value is string | null {
  return value === null || isText(value);
}

function isNullableNumber(value: Field): value is number | null {
  return value === null || isNumber(value);
}

const STATES: readonly string[] = [
  'stopped',
  'loading',
  'negotiating',
  'buffering',
  'playing',
  'paused',
  'error',
];

function isAppInfo(value: JsonValue): value is JsonValue & AppInfo {
  return (
    isRecord(value) &&
    isText(value.name) &&
    isText(value.version) &&
    isText(value.platform)
  );
}

function isDevice(value: JsonValue): value is JsonValue & DeviceInfo {
  return (
    isRecord(value) &&
    isText(value.name) &&
    isText(value.cardName) &&
    isText(value.pcmName) &&
    isFlag(value.usb)
  );
}

function isDeviceList(value: JsonValue): value is JsonValue & DeviceInfo[] {
  return Array.isArray(value) && value.every(isDevice);
}

function isDroppedFile(value: JsonValue): value is JsonValue & DroppedFile {
  return isRecord(value) && isText(value.path) && isText(value.name);
}

function isVerdict(value: Field): value is JsonValue & Verdict {
  return (
    isRecord(value) &&
    (value.status === 'yes' ||
      value.status === 'no' ||
      value.status === 'unverified') &&
    isNullableText(value.reason)
  );
}

function isOutput(value: Field): value is JsonValue & Output {
  return (
    isRecord(value) &&
    (value.transport === 'pcm' ||
      value.transport === 'dop' ||
      value.transport === 'native') &&
    isText(value.format) &&
    isNumber(value.sampleRate) &&
    isNumber(value.channels)
  );
}

function isTrack(value: Field): value is JsonValue & Track {
  return (
    isRecord(value) &&
    isNullableNumber(value.libraryId) &&
    isNumber(value.queueIndex) &&
    isText(value.fileName) &&
    isText(value.device) &&
    (value.codec === 'flac' ||
      value.codec === 'wav' ||
      value.codec === 'dsf' ||
      value.codec === 'dff') &&
    isNumber(value.sampleRate) &&
    isNumber(value.bitsPerSample) &&
    isNullableNumber(value.dsdMultiple) &&
    isNumber(value.channels) &&
    isNullableNumber(value.durationSeconds) &&
    (value.output === null || isOutput(value.output)) &&
    (value.bitPerfect === null || isVerdict(value.bitPerfect))
  );
}

function isFolder(value: JsonValue): value is JsonValue & Folder {
  return isRecord(value) && isNumber(value.id) && isText(value.path);
}

function isFolderList(value: JsonValue): value is JsonValue & Folder[] {
  return Array.isArray(value) && value.every(isFolder);
}

function isSourceFormat(value: Readonly<Record<string, JsonValue>>): boolean {
  return (
    isText(value.codec) &&
    isNumber(value.sampleRate) &&
    isNullableNumber(value.dsdMultiple)
  );
}

function isAlbum(value: JsonValue): value is JsonValue & Album {
  return (
    isRecord(value) &&
    isSourceFormat(value) &&
    isNumber(value.id) &&
    isText(value.title) &&
    isText(value.artist) &&
    isNullableNumber(value.year) &&
    isNullableText(value.art) &&
    isNumber(value.trackCount)
  );
}

function isAlbumList(value: JsonValue): value is JsonValue & Album[] {
  return Array.isArray(value) && value.every(isAlbum);
}

function isLibraryTrack(value: JsonValue): value is JsonValue & LibraryTrack {
  return (
    isRecord(value) &&
    isSourceFormat(value) &&
    isNumber(value.id) &&
    isText(value.title) &&
    isText(value.artist) &&
    isText(value.album) &&
    isNumber(value.albumId) &&
    isNullableNumber(value.disc) &&
    isNullableNumber(value.track) &&
    isNullableNumber(value.durationSeconds) &&
    isNumber(value.bitsPerSample) &&
    isNumber(value.channels)
  );
}

function isLibraryTrackList(
  value: JsonValue,
): value is JsonValue & LibraryTrack[] {
  return Array.isArray(value) && value.every(isLibraryTrack);
}

function isArtist(value: JsonValue): value is JsonValue & Artist {
  return (
    isRecord(value) &&
    isNumber(value.id) &&
    isText(value.name) &&
    isNumber(value.albumCount) &&
    isNumber(value.trackCount)
  );
}

function isArtistList(value: JsonValue): value is JsonValue & Artist[] {
  return Array.isArray(value) && value.every(isArtist);
}

function isPlaylist(value: JsonValue): value is JsonValue & Playlist {
  return (
    isRecord(value) &&
    isNumber(value.id) &&
    isText(value.name) &&
    isNumber(value.trackCount) &&
    isNumber(value.durationSeconds)
  );
}

function isPlaylistList(value: JsonValue): value is JsonValue & Playlist[] {
  return Array.isArray(value) && value.every(isPlaylist);
}

function isPlaylistEntry(value: JsonValue): value is JsonValue & PlaylistEntry {
  return (
    isRecord(value) &&
    isNumber(value.entryId) &&
    value.track !== undefined &&
    isLibraryTrack(value.track)
  );
}

function isPlaylistEntries(
  value: JsonValue,
): value is JsonValue & PlaylistEntry[] {
  return Array.isArray(value) && value.every(isPlaylistEntry);
}

function isScanReport(value: Field): value is JsonValue & ScanReport {
  return (
    isRecord(value) &&
    isNumber(value.added) &&
    isNumber(value.updated) &&
    isNumber(value.removed) &&
    isNumber(value.failed)
  );
}

function isScanStatus(value: JsonValue): value is JsonValue & ScanStatus {
  return (
    isRecord(value) &&
    isFlag(value.running) &&
    isNumber(value.done) &&
    isNumber(value.total) &&
    (value.last === null || isScanReport(value.last)) &&
    isNullableText(value.error)
  );
}

function isQueueEntry(value: JsonValue): value is JsonValue & QueueEntry {
  return (
    isRecord(value) &&
    isNullableNumber(value.trackId) &&
    isText(value.title) &&
    isNullableText(value.artist) &&
    isNullableNumber(value.durationSeconds)
  );
}

function isQueue(value: JsonValue): value is JsonValue & QueueEntry[] {
  return Array.isArray(value) && value.every(isQueueEntry);
}

function isTrackDetails(
  value: JsonValue,
): value is JsonValue & (TrackDetails | null) {
  return (
    value === null ||
    (isRecord(value) &&
      isText(value.title) &&
      isText(value.artist) &&
      isNullableText(value.album) &&
      isNumber(value.albumId) &&
      isNullableNumber(value.year) &&
      isNullableText(value.art))
  );
}

function isIpcError(
  value: unknown,
): value is { code: string; message: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    typeof value.code === 'string' &&
    'message' in value &&
    typeof value.message === 'string'
  );
}

function isStatus(value: JsonValue): value is JsonValue & PlayerStatus {
  return (
    isRecord(value) &&
    isText(value.state) &&
    STATES.includes(value.state) &&
    (value.track === null || isTrack(value.track)) &&
    (value.error === null || isIpcError(value.error)) &&
    isNumber(value.queueLength)
  );
}

function isNothing(value: JsonValue): value is null {
  return value === null;
}

function hasMessage(value: unknown): value is { message: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'message' in value &&
    typeof value.message === 'string'
  );
}

function isRejectionText(value: unknown): value is string {
  return typeof value === 'string';
}

/** Tauri rejects with our `{ code, message }`, a string, or an error object. */
function toBackendError(cause: unknown): BackendError {
  if (isIpcError(cause)) return new BackendError(cause.code, cause.message);

  if (isRejectionText(cause)) {
    return new BackendError('command_failed', cause, cause);
  }

  if (hasMessage(cause)) {
    return new BackendError('command_failed', cause.message, cause);
  }

  return new BackendError(
    'command_failed',
    'Could not connect to Rust. Try checking the backend again.',
    cause,
  );
}

/** Calls a command and checks the response shape. */
async function call<T>(
  command: string,
  guard: Guard<T>,
  args: CommandArgs | undefined,
  runCommand: RunCommand,
  inTauri: boolean,
): Promise<T> {
  if (!inTauri) {
    throw new BackendError(
      'browser_preview',
      'Browser preview. Open Lyra with Tauri to connect to Rust.',
    );
  }

  let response: JsonValue;

  try {
    response =
      args === undefined
        ? await runCommand(command)
        : await runCommand(command, args);
  } catch (cause) {
    throw toBackendError(cause);
  }

  if (!guard(response)) {
    throw new BackendError(
      'invalid_response',
      'Unexpected response from the Rust backend.',
    );
  }

  return response;
}

const UNEXPECTED = 'Unexpected error while contacting Rust.';

/** Runs a backend call; reports a readable message instead of throwing. */
export async function attempt<T>(
  action: () => Promise<T>,
  report: (message: string) => void,
): Promise<T | undefined> {
  try {
    return await action();
  } catch (error) {
    report(error instanceof BackendError ? error.message : UNEXPECTED);

    return undefined;
  }
}

/** Commands bound to one transport; tests pass a fake runner. */
export function backend(
  runCommand: RunCommand = invoke,
  inTauri: boolean = isTauri(),
) {
  const run = <T>(command: string, guard: Guard<T>, args?: CommandArgs) =>
    call(command, guard, args, runCommand, inTauri);

  return {
    appInfo: () => run('get_app_info', isAppInfo),
    listDevices: () => run('list_devices', isDeviceList),
    play: (path: string, device: string) =>
      run('play', isNothing, { path, device }),
    pause: () => run('pause', isNothing),
    resume: () => run('resume', isNothing),
    stop: () => run('stop', isNothing),
    seek: (seconds: number) => run('seek', isNothing, { seconds }),
    status: () => run('get_status', isStatus),
    position: () => run('get_position', isNumber),
    playTracks: (trackIds: number[], start: number, device: string) =>
      run('play_tracks', isNothing, { trackIds, start, device }),
    next: () => run('next', isNothing),
    previous: () => run('previous', isNothing),
    skipTo: (index: number) => run('skip_to', isNothing, { index }),
    queue: () => run('get_queue', isQueue),
    trackDetails: (trackId: number) =>
      run<TrackDetails | null>('track_details', isTrackDetails, { trackId }),
    folders: () => run('list_folders', isFolderList),
    pickFolder: () => run('pick_folder', isNothing),
    removeFolder: (id: number) => run('remove_folder', isNothing, { id }),
    rescan: () => run('rescan', isNothing),
    scanStatus: () => run('get_scan_status', isScanStatus),
    albums: () => run('list_albums', isAlbumList),
    albumTracks: (albumId: number) =>
      run('album_tracks', isLibraryTrackList, { albumId }),
    artists: () => run('list_artists', isArtistList),
    artistAlbums: (artistId: number) =>
      run('artist_albums', isAlbumList, { artistId }),
    tracks: () => run('list_tracks', isLibraryTrackList),
    playlists: () => run('list_playlists', isPlaylistList),
    createPlaylist: (name: string) =>
      run('create_playlist', isNumber, { name }),
    renamePlaylist: (id: number, name: string) =>
      run('rename_playlist', isNothing, { id, name }),
    deletePlaylist: (id: number) => run('delete_playlist', isNothing, { id }),
    playlistEntries: (id: number) =>
      run('playlist_entries', isPlaylistEntries, { id }),
    addToPlaylist: (id: number, trackIds: number[]) =>
      run('add_to_playlist', isNothing, { id, trackIds }),
    removeFromPlaylist: (entryId: number) =>
      run('remove_from_playlist', isNothing, { entryId }),
    moveInPlaylist: (entryId: number, to: number) =>
      run('move_in_playlist', isNothing, { entryId, to }),
  };
}

export type Backend = ReturnType<typeof backend>;

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

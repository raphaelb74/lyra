import { invoke, isTauri } from '@tauri-apps/api/core';
import {
  isNumber,
  isAppInfo,
  isDeviceList,
  isVolume,
  isFolderList,
  isAlbumList,
  isLibraryTrackList,
  isArtistList,
  isPlaylistList,
  isPlaylistEntries,
  isScanStatus,
  isQueue,
  isTrackDetails,
  isIpcError,
  isStatus,
  isNothing,
  isSettings,
  isDiagnostics,
  isCapabilities,
  isAlbumEditor,
  isFlag,
  isProposalList,
  isCandidateList,
  isMatchInfo,
  isCover,
  type Guard,
} from './guards';

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
  /** Short name for people, e.g. `iFi USB Audio SE`, `HDMI 2`. */
  label: string;
  /** Long ALSA card name; for diagnostics. */
  cardName: string;
  pcmName: string;
  usb: boolean;
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
  /** `converted`: DSD converted to PCM, not bit-perfect. */
  transport: 'pcm' | 'dop' | 'native' | 'converted';
  format: string;
  sampleRate: number;
  channels: number;
  /** The device's own volume; `unity` is 0 dB, `unknown` has no dB scale. */
  hardwareVolume: 'none' | 'unity' | 'attenuated' | 'unknown';
  /** Set when `attenuated`. */
  hardwareDb: number | null;
}

/** One thing UNDA does to the samples on their way to the device. */
export type Step =
  /** PCM in a wider sample format, extra low bits zero: lossless. */
  | { kind: 'padded'; fromBits: number; toBits: number }
  /** DSD in PCM frames with DoP markers: lossless for a DoP DAC. */
  | { kind: 'dopPacked' }
  /** DSD converted to PCM with this gain: not bit-perfect. */
  | { kind: 'converted'; gainDb: number };

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
  /** In order; empty when the samples go as read. */
  processing: Step[];
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
  /** A small copy of the cover, for the grid. */
  thumb: string | null;
  trackCount: number;
  genre: string | null;
  durationSeconds: number | null;
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
  /** `lyra-art://` URL of a cover standing in for a photo, or null. */
  art: string | null;
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
  /** Library folders that could not be read; their tracks were kept. */
  unavailable: string[];
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
  /** The queue plays in shuffled order (the Queue view shows that order). */
  shuffle: boolean;
  repeat: Repeat;
  /** Changes whenever the queue does; the queue pane reloads on it. */
  queueRevision: number;
  /** Next has a track to go to (a later entry, or the start when repeating). */
  hasNext: boolean;
}

/** What plays after a track ends: stop at the end, the queue again, the track again. */
export type Repeat = 'off' | 'all' | 'one';

/** Mirrors Rust's `QueueEntry`. */
export interface QueueEntry {
  trackId: number | null;
  title: string;
  artist: string | null;
  durationSeconds: number | null;
  /** Album cover URL. */
  art: string | null;
}

/** Mirrors Rust's `TrackDetails`: library data for Now Playing. */
export interface TrackDetails {
  title: string;
  artist: string;
  album: string | null;
  albumId: number;
  year: number | null;
  art: string | null;
  /** A small copy of the cover, for the player bar. */
  thumb: string | null;
}

/** Errors raised in the UI itself, plus the `code`s Rust sends. */
/** Mirrors Rust's `VolumeInfo`: a card's hardware volume. */
export interface VolumeInfo {
  percent: number;
  decibels: number | null;
  /** USB DAC: below 0 dB the samples usually change. */
  usb: boolean;
  /** Why the volume cannot change now (DoP playing). */
  locked: string | null;
}

/** Mirrors Rust's `Settings`. */
export interface Settings {
  dsdTransport: 'auto' | 'native' | 'dop';
  /** DSD on devices without DSD support. */
  dsdFallback: 'refuse' | 'convert';
  /** Gain for converted DSD: 0, 3 or 6 dB. */
  conversionGainDb: number;
  /** Silence before playback when the format changes: 0–2000 ms. */
  resyncDelayMs: number;
  /** Devices (`hw:CARD=…,DEV=…`) the user says decode DoP. */
  dopDevices: string[];
  /** ID Match may ask MusicBrainz and the Cover Art Archive. Off by default. */
  onlineLookups: boolean;
}

/** Mirrors Rust's `DiagnosticsInfo`: one device, read now. */
export interface DiagnosticsInfo {
  device: string;
  kernel: string | null;
  alsa: string | null;
  usbId: string | null;
  /** The kernel's `hw_params` while anyone has the device open. */
  hwParams: string | null;
  /** USB stream state while playing, with the DAC's feedback rate. */
  usbStream: string | null;
  /** Present while Lyra plays on this device. */
  playback: PlaybackDiagnostics | null;
}

export interface PlaybackDiagnostics {
  /** Since the device was opened. */
  underruns: number;
  bufferFrames: number;
  periodFrames: number;
  /** Program that handed the device to Lyra, e.g. `pipewire`. */
  takenFrom: string | null;
  conversion: ConversionInfo | null;
}

export interface ConversionInfo {
  clipped: number;
  /** dBFS before clamping; null while only silence played. */
  peakDb: number | null;
}

/** Mirrors Rust's `CapabilitiesInfo`: what the driver says it accepts. */
export interface Capabilities {
  channelsMin: number;
  channelsMax: number;
  formats: { format: string; rates: number[] }[];
}

/** A track or album, as edits name them. */
export interface TargetRef {
  kind: 'track' | 'release';
  id: number;
}

/** Mirrors Rust's `FieldInfo`: one field in the editor. */
export interface FieldInfo {
  /** Stable name, sent back in a change. */
  field: string;
  label: string;
  /** In order; empty when nothing is shown. */
  values: string[];
  /** May hold several values. */
  multiple: boolean;
  /** An edit is in force over the files. */
  edited: boolean;
  /** The file changed under the edit. */
  review: boolean;
}

export interface EditorTrack {
  id: number;
  fileName: string;
  fields: FieldInfo[];
}

export interface RevisionInfo {
  target: TargetRef;
  revision: number;
}

/** Mirrors Rust's `AlbumEditor`. */
export interface AlbumEditor {
  albumId: number;
  art: string | null;
  fields: FieldInfo[];
  tracks: EditorTrack[];
  /** What a save is checked against. */
  revisions: RevisionInfo[];
}

export type FieldEdit =
  { kind: 'set'; values: string[] } | { kind: 'clear' } | { kind: 'reset' };

export interface FieldChange {
  target: TargetRef;
  field: string;
  edit: FieldEdit;
}

/** Mirrors Rust's `EvidenceInfo`: what agrees between album and edition. */
export interface Evidence {
  releaseId: boolean;
  trackCount: boolean;
  title: boolean;
  artist: boolean;
  year: boolean;
}

/** Mirrors Rust's `CandidateInfo`: one edition ID Match found. */
export interface Candidate {
  releaseId: string;
  title: string;
  artist: string;
  disambiguation: string | null;
  date: string | null;
  country: string | null;
  status: string | null;
  /** `Label catalogue-number`. */
  labels: string[];
  /** `2×CD`, `12" Vinyl`, … */
  formats: string[];
  trackCount: number | null;
  evidence: Evidence;
}

/** A release track: its medium and place, from 0. */
export interface Slot {
  medium: number;
  index: number;
}

export interface ReleaseTrack {
  /** As printed: `3`, `A1`. */
  number: string;
  title: string;
  seconds: number | null;
}

export interface Medium {
  position: number;
  format: string | null;
  title: string | null;
  tracks: ReleaseTrack[];
}

export interface MatchTrack {
  id: number;
  disc: string | null;
  number: string | null;
  title: string | null;
  seconds: number | null;
}

export type PairedBy = 'id' | 'number' | 'runningNumber' | 'order' | 'hand';

export interface Pair {
  track: number;
  slot: Slot;
  by: PairedBy;
  /** Album length minus release length. */
  lengthDifferenceSeconds: number | null;
  lengthsAgree: boolean | null;
}

export interface Suggestion {
  target: TargetRef;
  field: string;
  fieldLabel: string;
  before: string[];
  after: string[];
  /** The listener edited this field; that edit is kept. */
  kept: boolean;
}

/** Mirrors Rust's `MatchInfo`: an edition compared with the album. */
export interface MatchInfo {
  release: Candidate;
  /** The release's page on MusicBrainz. */
  url: string;
  media: Medium[];
  tracks: MatchTrack[];
  pairs: Pair[];
  unpaired: number[];
  missing: Slot[];
  suggestions: Suggestion[];
  /** What a save is checked against. */
  revisions: RevisionInfo[];
}

/** A pair the listener chose. */
export interface PairChoice {
  track: number;
  slot: Slot;
}

/** A suggestion the listener accepted. */
export interface FieldKey {
  target: TargetRef;
  field: string;
}

/** Mirrors Rust's `ProposalInfo`: a fix the cleanup rules propose. */
export interface Proposal {
  target: TargetRef;
  field: string;
  fieldLabel: string;
  /** `Album`, or `Track — Album`. */
  item: string;
  before: string[];
  after: string[];
  rules: { name: string; label: string }[];
}

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
    /** Plays queue entry `index`; a stopped queue starts again on `device`. */
    skipTo: (index: number, device: string) =>
      run('skip_to', isNothing, { index, device }),
    /** After the playing track (`next`) or at the end; plays them when nothing plays. */
    enqueue: (trackIds: number[], next: boolean, device: string) =>
      run('enqueue_tracks', isNothing, { trackIds, next, device }),
    /** Entry `index` plays right after entry `after`; next when null. */
    queueMove: (index: number, after: number | null) =>
      run('queue_move', isNothing, { index, after }),
    queueRemove: (index: number) => run('queue_remove', isNothing, { index }),
    clearQueue: () => run('clear_queue', isNothing),
    setShuffle: (on: boolean) => run('set_shuffle', isNothing, { on }),
    setRepeat: (mode: Repeat) => run('set_repeat', isNothing, { mode }),
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
    volume: (device: string) =>
      run<VolumeInfo | null>('get_volume', isVolume, { device }),
    setVolume: (device: string, percent: number) =>
      run<VolumeInfo | null>('set_volume', isVolume, { device, percent }),
    settings: () => run('get_settings', isSettings),
    diagnostics: (device: string) =>
      run('get_diagnostics', isDiagnostics, { device }),
    probeDevice: (device: string) =>
      run('probe_device', isCapabilities, { device }),
    albumEditor: (albumId: number) =>
      run('album_editor', isAlbumEditor, { albumId }),
    /** Saves in the library only; fails with `changed_meanwhile` when stale. */
    saveAlbumEdits: (changes: FieldChange[], revisions: RevisionInfo[]) =>
      run('save_album_edits', isNothing, {
        changes: changes.map((change) => ({
          target: { ...change.target },
          field: change.field,
          edit: { ...change.edit },
        })),
        revisions: revisions.map((revision) => ({
          target: { ...revision.target },
          revision: revision.revision,
        })),
      }),
    /** `false` when there was nothing to undo. */
    undoEdit: () => run('undo_edit', isFlag),
    /** Opens the image picker; the result arrives as a library event. */
    chooseArtwork: (albumId: number) =>
      run('choose_artwork', isNothing, { albumId }),
    resetArtwork: (albumId: number) =>
      run('reset_artwork', isNothing, { albumId }),
    /** Proposed fixes for one album, or the whole library when null. */
    proposeFixes: (albumId: number | null) =>
      run('propose_fixes', isProposalList, { albumId }),
    applyFixes: (proposals: Proposal[]) =>
      run('apply_fixes', isNothing, {
        accepted: proposals.map((p) => ({
          target: { ...p.target },
          field: p.field,
          values: p.after,
          rules: p.rules.map((rule) => rule.name),
        })),
      }),
    /**
     * Editions for an album: automatic without a title, else for what was
     * typed. Fails with `online_off` while online lookups are off.
     */
    matchCandidates: (
      albumId: number,
      title: string | null = null,
      artist: string | null = null,
    ) => run('match_candidates', isCandidateList, { albumId, title, artist }),
    /** One edition compared with the album; `pairs` as chosen by hand. */
    matchRelease: (
      albumId: number,
      releaseId: string,
      pairs: PairChoice[] | null = null,
    ) =>
      run('match_release', isMatchInfo, {
        albumId,
        releaseId,
        pairs: pairs && copyPairs(pairs),
      }),
    /** The edition's front cover as a `data:` URL, or null. */
    matchCover: (releaseId: string) =>
      run<string | null>('match_cover', isCover, { releaseId }),
    /**
     * Saves the accepted suggestions (values come from MusicBrainz, in
     * Rust) and the cover if asked; `changed_meanwhile` when stale.
     */
    applyMatch: (
      albumId: number,
      releaseId: string,
      pairs: PairChoice[] | null,
      accepted: FieldKey[],
      revisions: RevisionInfo[],
      cover: boolean,
    ) =>
      run('apply_match', isNothing, {
        albumId,
        releaseId,
        pairs: pairs && copyPairs(pairs),
        accepted: accepted.map((key) => ({
          target: { ...key.target },
          field: key.field,
        })),
        revisions: revisions.map((revision) => ({
          target: { ...revision.target },
          revision: revision.revision,
        })),
        cover,
      }),
    setSettings: (settings: Settings) =>
      run('set_settings', isSettings, { settings: { ...settings } }),
  };
}

export type Backend = ReturnType<typeof backend>;

/** Plain copies: reactive proxies do not cross IPC. */
function copyPairs(pairs: PairChoice[]) {
  return pairs.map((pair) => ({
    track: pair.track,
    slot: { medium: pair.slot.medium, index: pair.slot.index },
  }));
}

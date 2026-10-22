/*
 * Checks for every value that crosses the IPC boundary: nothing from
 * Rust is trusted by shape alone.
 */

import type {
  Album,
  AppInfo,
  Artist,
  Capabilities,
  ConversionInfo,
  DiagnosticsInfo,
  DeviceInfo,
  Folder,
  JsonValue,
  LibraryTrack,
  Output,
  PlaybackDiagnostics,
  PlayerStatus,
  Playlist,
  PlaylistEntry,
  QueueEntry,
  ScanReport,
  ScanStatus,
  Settings,
  Track,
  TrackDetails,
  Verdict,
  VolumeInfo,
} from './ipc';

export type Guard<T> = (value: JsonValue) => value is JsonValue & T;

/** A field read from a record may be absent. */
type Field = JsonValue | undefined;

export function isRecord(
  value: Field,
): value is Readonly<Record<string, JsonValue>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function isText(value: Field): value is string {
  return typeof value === 'string';
}

export function isNumber(value: Field): value is number {
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

export function isAppInfo(value: JsonValue): value is JsonValue & AppInfo {
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
    isText(value.label) &&
    isText(value.cardName) &&
    isText(value.pcmName) &&
    isFlag(value.usb)
  );
}

export function isDeviceList(
  value: JsonValue,
): value is JsonValue & DeviceInfo[] {
  return Array.isArray(value) && value.every(isDevice);
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

export function isVolume(
  value: Field,
): value is (JsonValue & VolumeInfo) | null {
  return (
    value === null ||
    (isRecord(value) &&
      isNumber(value.percent) &&
      isNullableNumber(value.decibels) &&
      isFlag(value.usb) &&
      isNullableText(value.locked))
  );
}

export function isSettings(value: Field): value is JsonValue & Settings {
  return (
    isRecord(value) &&
    (value.dsdTransport === 'auto' ||
      value.dsdTransport === 'native' ||
      value.dsdTransport === 'dop') &&
    (value.dsdFallback === 'refuse' || value.dsdFallback === 'convert') &&
    isNumber(value.conversionGainDb) &&
    isNumber(value.resyncDelayMs)
  );
}

function isConversion(value: Field): value is JsonValue & ConversionInfo {
  return (
    isRecord(value) && isNumber(value.clipped) && isNullableNumber(value.peakDb)
  );
}

function isPlayback(value: Field): value is JsonValue & PlaybackDiagnostics {
  return (
    isRecord(value) &&
    isNumber(value.underruns) &&
    isNumber(value.bufferFrames) &&
    isNumber(value.periodFrames) &&
    isNullableText(value.takenFrom) &&
    (value.conversion === null || isConversion(value.conversion))
  );
}

export function isDiagnostics(
  value: Field,
): value is JsonValue & DiagnosticsInfo {
  return (
    isRecord(value) &&
    isText(value.device) &&
    isNullableText(value.kernel) &&
    isNullableText(value.alsa) &&
    isNullableText(value.usbId) &&
    isNullableText(value.hwParams) &&
    isNullableText(value.usbStream) &&
    (value.playback === null || isPlayback(value.playback))
  );
}

export function isCapabilities(
  value: Field,
): value is JsonValue & Capabilities {
  return (
    isRecord(value) &&
    isNumber(value.channelsMin) &&
    isNumber(value.channelsMax) &&
    Array.isArray(value.formats) &&
    value.formats.every(isFormatSupport)
  );
}

function isFormatSupport(
  value: Field,
): value is JsonValue & Capabilities['formats'][number] {
  return (
    isRecord(value) &&
    isText(value.format) &&
    Array.isArray(value.rates) &&
    value.rates.every(isNumber)
  );
}

function isOutput(value: Field): value is JsonValue & Output {
  return (
    isRecord(value) &&
    (value.transport === 'pcm' ||
      value.transport === 'dop' ||
      value.transport === 'native' ||
      value.transport === 'converted') &&
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

export function isFolderList(value: JsonValue): value is JsonValue & Folder[] {
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

export function isAlbumList(value: JsonValue): value is JsonValue & Album[] {
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

export function isLibraryTrackList(
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
    isNumber(value.trackCount) &&
    isNullableText(value.art)
  );
}

export function isArtistList(value: JsonValue): value is JsonValue & Artist[] {
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

export function isPlaylistList(
  value: JsonValue,
): value is JsonValue & Playlist[] {
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

export function isPlaylistEntries(
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

export function isScanStatus(
  value: JsonValue,
): value is JsonValue & ScanStatus {
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

export function isQueue(value: JsonValue): value is JsonValue & QueueEntry[] {
  return Array.isArray(value) && value.every(isQueueEntry);
}

export function isTrackDetails(
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

export function isIpcError(
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

export function isStatus(value: JsonValue): value is JsonValue & PlayerStatus {
  return (
    isRecord(value) &&
    isText(value.state) &&
    STATES.includes(value.state) &&
    (value.track === null || isTrack(value.track)) &&
    (value.error === null || isIpcError(value.error)) &&
    isNumber(value.queueLength)
  );
}

export function isNothing(value: JsonValue): value is null {
  return value === null;
}

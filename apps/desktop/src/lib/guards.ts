/*
 * Checks for every value that crosses the IPC boundary: nothing from
 * Rust is trusted by shape alone.
 */

import type {
  Album,
  AlbumEditor,
  Candidate,
  MatchInfo,
  EditorTrack,
  FieldInfo,
  Proposal,
  TargetRef,
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
  Step,
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

export function isFlag(value: Field): value is boolean {
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

const REPEATS: readonly string[] = ['off', 'all', 'one'];

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
    isNumber(value.resyncDelayMs) &&
    Array.isArray(value.dopDevices) &&
    value.dopDevices.every(isText) &&
    isFlag(value.onlineLookups)
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
    isNumber(value.channels) &&
    (value.hardwareVolume === 'none' ||
      value.hardwareVolume === 'unity' ||
      value.hardwareVolume === 'attenuated' ||
      value.hardwareVolume === 'unknown') &&
    isNullableNumber(value.hardwareDb)
  );
}

function isStep(value: Field): value is JsonValue & Step {
  if (!isRecord(value)) return false;

  switch (value.kind) {
    case 'padded':
      return isNumber(value.fromBits) && isNumber(value.toBits);
    case 'dopPacked':
      return true;
    case 'converted':
      return isNumber(value.gainDb);
    default:
      return false;
  }
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
    Array.isArray(value.processing) &&
    value.processing.every(isStep) &&
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
    isNullableText(value.thumb) &&
    isNumber(value.trackCount) &&
    isNullableText(value.genre) &&
    isNullableNumber(value.durationSeconds)
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
    isNumber(value.failed) &&
    Array.isArray(value.unavailable) &&
    value.unavailable.every(isText)
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
    isNullableNumber(value.durationSeconds) &&
    isNullableText(value.art)
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
      isNullableText(value.art) &&
      isNullableText(value.thumb))
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
    isNumber(value.queueLength) &&
    isNumber(value.queueRevision) &&
    typeof value.hasNext === 'boolean' &&
    typeof value.shuffle === 'boolean' &&
    isText(value.repeat) &&
    REPEATS.includes(value.repeat)
  );
}

export function isNothing(value: JsonValue): value is null {
  return value === null;
}

function isTextList(value: Field): value is string[] {
  return Array.isArray(value) && value.every(isText);
}

function isTarget(value: Field): value is JsonValue & TargetRef {
  return (
    isRecord(value) &&
    (value.kind === 'track' || value.kind === 'release') &&
    isNumber(value.id)
  );
}

function isFieldInfo(value: JsonValue): value is JsonValue & FieldInfo {
  return (
    isRecord(value) &&
    isText(value.field) &&
    isText(value.label) &&
    isTextList(value.values) &&
    isFlag(value.multiple) &&
    isFlag(value.edited) &&
    isFlag(value.review)
  );
}

function isFieldList(value: Field): value is JsonValue & FieldInfo[] {
  return Array.isArray(value) && value.every(isFieldInfo);
}

function isEditorTrack(value: JsonValue): value is JsonValue & EditorTrack {
  return (
    isRecord(value) &&
    isNumber(value.id) &&
    isText(value.fileName) &&
    isFieldList(value.fields)
  );
}

export function isAlbumEditor(
  value: JsonValue,
): value is JsonValue & AlbumEditor {
  return (
    isRecord(value) &&
    isNumber(value.albumId) &&
    isNullableText(value.art) &&
    isFieldList(value.fields) &&
    Array.isArray(value.tracks) &&
    value.tracks.every(isEditorTrack) &&
    Array.isArray(value.revisions) &&
    value.revisions.every(isRevision)
  );
}

function isRevision(value: JsonValue): boolean {
  return isRecord(value) && isTarget(value.target) && isNumber(value.revision);
}

function isRule(value: JsonValue): boolean {
  return isRecord(value) && isText(value.name) && isText(value.label);
}

function isProposal(value: JsonValue): value is JsonValue & Proposal {
  return (
    isRecord(value) &&
    isTarget(value.target) &&
    isText(value.field) &&
    isText(value.fieldLabel) &&
    isText(value.item) &&
    isTextList(value.before) &&
    isTextList(value.after) &&
    Array.isArray(value.rules) &&
    value.rules.every(isRule)
  );
}

export function isProposalList(
  value: JsonValue,
): value is JsonValue & Proposal[] {
  return Array.isArray(value) && value.every(isProposal);
}

function isEvidence(value: Field): boolean {
  return (
    isRecord(value) &&
    isFlag(value.releaseId) &&
    isFlag(value.trackCount) &&
    isFlag(value.title) &&
    isFlag(value.artist) &&
    isFlag(value.year)
  );
}

function isCandidate(value: Field): value is JsonValue & Candidate {
  return (
    isRecord(value) &&
    isText(value.releaseId) &&
    isText(value.title) &&
    isText(value.artist) &&
    isNullableText(value.disambiguation) &&
    isNullableText(value.date) &&
    isNullableText(value.country) &&
    isNullableText(value.status) &&
    isTextList(value.labels) &&
    isTextList(value.formats) &&
    isNullableNumber(value.trackCount) &&
    isEvidence(value.evidence)
  );
}

export function isCandidateList(
  value: JsonValue,
): value is JsonValue & Candidate[] {
  return Array.isArray(value) && value.every(isCandidate);
}

function isSlot(value: Field): boolean {
  return isRecord(value) && isNumber(value.medium) && isNumber(value.index);
}

const PAIRED_BY: readonly string[] = [
  'id',
  'number',
  'runningNumber',
  'order',
  'hand',
];

function isPair(value: JsonValue): boolean {
  return (
    isRecord(value) &&
    isNumber(value.track) &&
    isSlot(value.slot) &&
    isText(value.by) &&
    PAIRED_BY.includes(value.by) &&
    isNullableNumber(value.lengthDifferenceSeconds) &&
    (value.lengthsAgree === null || isFlag(value.lengthsAgree))
  );
}

function isReleaseTrack(value: JsonValue): boolean {
  return (
    isRecord(value) &&
    isText(value.number) &&
    isText(value.title) &&
    isNullableNumber(value.seconds)
  );
}

function isMedium(value: JsonValue): boolean {
  return (
    isRecord(value) &&
    isNumber(value.position) &&
    isNullableText(value.format) &&
    isNullableText(value.title) &&
    Array.isArray(value.tracks) &&
    value.tracks.every(isReleaseTrack)
  );
}

function isMatchTrack(value: JsonValue): boolean {
  return (
    isRecord(value) &&
    isNumber(value.id) &&
    isNullableText(value.disc) &&
    isNullableText(value.number) &&
    isNullableText(value.title) &&
    isNullableNumber(value.seconds)
  );
}

function isSuggestion(value: JsonValue): boolean {
  return (
    isRecord(value) &&
    isTarget(value.target) &&
    isText(value.field) &&
    isText(value.fieldLabel) &&
    isTextList(value.before) &&
    isTextList(value.after) &&
    isFlag(value.kept)
  );
}

export function isMatchInfo(value: JsonValue): value is JsonValue & MatchInfo {
  return (
    isRecord(value) &&
    isCandidate(value.release) &&
    isText(value.url) &&
    Array.isArray(value.media) &&
    value.media.every(isMedium) &&
    Array.isArray(value.tracks) &&
    value.tracks.every(isMatchTrack) &&
    Array.isArray(value.pairs) &&
    value.pairs.every(isPair) &&
    Array.isArray(value.unpaired) &&
    value.unpaired.every(isNumber) &&
    Array.isArray(value.missing) &&
    value.missing.every(isSlot) &&
    Array.isArray(value.suggestions) &&
    value.suggestions.every(isSuggestion) &&
    Array.isArray(value.revisions) &&
    value.revisions.every(isRevision)
  );
}

/** A cover preview: an inline image, never a remote address. */
export function isCover(value: JsonValue): value is string | null {
  return (
    value === null ||
    (isText(value) && /^data:image\/(jpeg|png|webp);base64,/.test(value))
  );
}

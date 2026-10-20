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
  format: string;
  sampleRate: number;
  channels: number;
}

export interface Track {
  fileName: string;
  device: string;
  codec: 'flac' | 'wav';
  sampleRate: number;
  bitsPerSample: number;
  channels: number;
  durationSeconds: number | null;
  output: Output | null;
  bitPerfect: Verdict | null;
}

/** Mirrors Rust's `Status`. */
export interface PlayerStatus {
  state: PlaybackState;
  track: Track | null;
  error: { code: string; message: string } | null;
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
    isText(value.format) &&
    isNumber(value.sampleRate) &&
    isNumber(value.channels)
  );
}

function isTrack(value: Field): value is JsonValue & Track {
  return (
    isRecord(value) &&
    isText(value.fileName) &&
    isText(value.device) &&
    (value.codec === 'flac' || value.codec === 'wav') &&
    isNumber(value.sampleRate) &&
    isNumber(value.bitsPerSample) &&
    isNumber(value.channels) &&
    isNullableNumber(value.durationSeconds) &&
    (value.output === null || isOutput(value.output)) &&
    (value.bitPerfect === null || isVerdict(value.bitPerfect))
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
    (value.error === null || isIpcError(value.error))
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

import { invoke, isTauri } from '@tauri-apps/api/core';

/** Mirrors Rust's `AppInfo` (apps/desktop/src-tauri/src/ipc.rs). */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}

export type BackendErrorCode =
  'browser_preview' | 'command_failed' | 'invalid_response';

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

/** Sends one Tauri command; injectable so tests need no module mocking. */
export type RunCommand = (command: string) => Promise<JsonValue>;

function isAppInfo(value: JsonValue): value is JsonValue & AppInfo {
  return (
    typeof value === 'object' &&
    value !== null &&
    'name' in value &&
    typeof value.name === 'string' &&
    'version' in value &&
    typeof value.version === 'string' &&
    'platform' in value &&
    typeof value.platform === 'string'
  );
}

function hasMessage(value: unknown): value is { message: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'message' in value &&
    typeof value.message === 'string'
  );
}

function isText(value: unknown): value is string {
  return typeof value === 'string';
}

/** Tauri rejects with a string or a serialized error object. */
function rejectionMessage(cause: unknown): string {
  if (isText(cause)) return cause;

  if (hasMessage(cause)) return cause.message;

  return 'Could not connect to Rust. Try checking the backend again.';
}

export async function getAppInfo(
  runCommand: RunCommand = invoke,
  inTauri: boolean = isTauri(),
): Promise<AppInfo> {
  if (!inTauri) {
    throw new BackendError(
      'browser_preview',
      'Browser preview. Open Lyra with Tauri to connect to Rust.',
    );
  }

  let response: JsonValue;

  try {
    response = await runCommand('get_app_info');
  } catch (cause) {
    throw new BackendError('command_failed', rejectionMessage(cause), cause);
  }

  if (!isAppInfo(response)) {
    throw new BackendError(
      'invalid_response',
      'Unexpected response from the Rust backend.',
    );
  }

  return {
    name: response.name,
    version: response.version,
    platform: response.platform,
  };
}

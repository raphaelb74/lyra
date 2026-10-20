import { describe, expect, it, vi } from 'vitest';

import { backend, getAppInfo } from './ipc';

const info = { name: 'Lyra', version: '0.1.0', platform: 'linux' };

describe('desktop IPC boundary', () => {
  it('requests the registered command and accepts the Rust response shape', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(getAppInfo(runCommand, true)).resolves.toEqual(info);
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('get_app_info');
  });

  it('identifies browser preview without attempting IPC', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(getAppInfo(runCommand, false)).rejects.toMatchObject({
      code: 'browser_preview',
    });
    expect(runCommand).not.toHaveBeenCalled();
  });

  it.each([null, 'Lyra', {}, { name: 'Lyra', version: 1, platform: 'linux' }])(
    'rejects malformed backend data: %j',
    async (value) => {
      await expect(
        getAppInfo(() => Promise.resolve(value), true),
      ).rejects.toMatchObject({ code: 'invalid_response' });
    },
  );

  it.each([
    ['a string', 'get_app_info not allowed', 'get_app_info not allowed'],
    ['an error object', { message: 'Backend crashed' }, 'Backend crashed'],
    ['anything else', null, 'Try checking the backend again'],
  ])(
    'turns a rejection with %s into a readable error',
    async (_, cause, message) => {
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Tauri rejects with plain strings and objects; reproduce that.
      const result = getAppInfo(() => Promise.reject(cause), true);

      await expect(result).rejects.toThrow(message);
      await expect(result).rejects.toMatchObject({ code: 'command_failed' });
    },
  );
});

const status = {
  state: 'playing',
  track: {
    fileName: 'a.flac',
    device: 'hw:CARD=ONE,DEV=0',
    codec: 'flac',
    sampleRate: 96000,
    bitsPerSample: 24,
    channels: 2,
    durationSeconds: 30,
    output: { format: 'S24_3LE', sampleRate: 96000, channels: 2 },
    bitPerfect: { status: 'yes', reason: null },
  },
  error: null,
};

describe('playback commands', () => {
  it('sends play with the dropped path and device', async () => {
    const runCommand = vi.fn(() => Promise.resolve(null));

    await backend(runCommand, true).play('/m/a.flac', 'hw:CARD=ONE,DEV=0');
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('play', {
      path: '/m/a.flac',
      device: 'hw:CARD=ONE,DEV=0',
    });
  });

  it('keeps the code and message of a Rust IpcError', async () => {
    const busy = {
      code: 'device_busy',
      message: 'hw:CARD=ONE,DEV=0 is in use by pipewire (pid 1283)',
    };

    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Tauri rejects with the serialized IpcError object.
    const result = backend(() => Promise.reject(busy), true).play('a', 'b');

    await expect(result).rejects.toMatchObject(busy);
  });

  it('accepts a full status and rejects a malformed one', async () => {
    await expect(
      backend(() => Promise.resolve(status), true).status(),
    ).resolves.toEqual(status);
    await expect(
      backend(
        () => Promise.resolve({ ...status, state: 'exploding' }),
        true,
      ).status(),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });

  it('validates device lists', async () => {
    const devices = [
      {
        name: 'hw:CARD=ONE,DEV=0',
        cardName: 'ZEN ONE',
        pcmName: 'USB Audio',
        usb: true,
      },
    ];

    await expect(
      backend(() => Promise.resolve(devices), true).listDevices(),
    ).resolves.toEqual(devices);
    await expect(
      backend(() => Promise.resolve([{ name: 1 }]), true).listDevices(),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });
});

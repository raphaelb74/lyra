import { describe, expect, it, vi } from 'vitest';

import { backend } from './ipc';

const info = { name: 'Lyra', version: '0.1.0', platform: 'linux' };

describe('desktop IPC boundary', () => {
  it('requests the registered command and accepts the Rust response shape', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(backend(runCommand, true).appInfo()).resolves.toEqual(info);
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('get_app_info');
  });

  it('identifies browser preview without attempting IPC', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(backend(runCommand, false).appInfo()).rejects.toMatchObject({
      code: 'browser_preview',
    });
    expect(runCommand).not.toHaveBeenCalled();
  });

  it.each([null, 'Lyra', {}, { name: 'Lyra', version: 1, platform: 'linux' }])(
    'rejects malformed backend data: %j',
    async (value) => {
      await expect(
        backend(() => Promise.resolve(value), true).appInfo(),
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
      const result = backend(() => Promise.reject(cause), true).appInfo();

      await expect(result).rejects.toThrow(message);
      await expect(result).rejects.toMatchObject({ code: 'command_failed' });
    },
  );
});

const status = {
  state: 'playing',
  track: {
    libraryId: 12,
    queueIndex: 0,
    fileName: 'a.flac',
    device: 'hw:CARD=ONE,DEV=0',
    codec: 'flac',
    sampleRate: 96000,
    bitsPerSample: 24,
    dsdMultiple: null,
    channels: 2,
    durationSeconds: 30,
    output: {
      transport: 'pcm',
      format: 'S24_3LE',
      sampleRate: 96000,
      channels: 2,
    },
    bitPerfect: { status: 'yes', reason: null },
  },
  error: null,
  queueLength: 3,
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

  it('validates library albums and passes track ids to play', async () => {
    const album = {
      id: 1,
      title: 'Aladdin Sane',
      artist: 'David Bowie',
      year: 1973,
      art: 'lyra-art://localhost/1-1.jpg',
      trackCount: 10,
      codec: 'dsf',
      sampleRate: 2822400,
      dsdMultiple: 64,
    };

    const runCommand = vi.fn((command: string) =>
      Promise.resolve(command === 'list_albums' ? [album] : null),
    );

    const api = backend(runCommand, true);

    await expect(api.albums()).resolves.toEqual([album]);

    await api.playTracks([7, 8, 9], 1, 'hw:CARD=ONE,DEV=0');
    expect(runCommand).toHaveBeenLastCalledWith('play_tracks', {
      trackIds: [7, 8, 9],
      start: 1,
      device: 'hw:CARD=ONE,DEV=0',
    });

    await expect(
      backend(() => Promise.resolve([{ ...album, id: '1' }]), true).albums(),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });
});

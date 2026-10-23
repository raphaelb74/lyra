import { describe, expect, it, vi } from 'vitest';

import { backend, type JsonValue, type Settings } from './ipc';

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
      hardwareVolume: 'unity',
      hardwareDb: null,
    },
    processing: [],
    bitPerfect: { status: 'yes', reason: null },
  },
  error: null,
  queueLength: 3,
  shuffle: false,
  repeat: 'off',
  queueRevision: 1,
  hasNext: true,
};

describe('playback commands', () => {
  it('keeps the code and message of a Rust IpcError', async () => {
    const busy = {
      code: 'device_busy',
      message: 'hw:CARD=ONE,DEV=0 is in use by pipewire (pid 1283)',
    };

    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Tauri rejects with the serialized IpcError object.
    const result = backend(() => Promise.reject(busy), true).next();

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
        label: 'ZEN ONE',
        cardName: 'iFi (by AMR) ZEN One at usb-0000:00:14.0-2, high speed',
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
      thumb: 'lyra-art://localhost/thumb/1-1.jpg',
      trackCount: 10,
      genre: 'Rock',
      durationSeconds: 2496,
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

  it('validates playlist entries down to their track', async () => {
    const track = {
      id: 3,
      title: 'Watch That Man',
      artist: 'David Bowie',
      album: 'Aladdin Sane',
      albumId: 1,
      disc: null,
      track: 1,
      durationSeconds: 269.5,
      codec: 'dsf',
      sampleRate: 2822400,
      dsdMultiple: 64,
      bitsPerSample: 1,
      channels: 2,
    };

    const entries = [{ entryId: 9, track }];

    await expect(
      backend(() => Promise.resolve(entries), true).playlistEntries(1),
    ).resolves.toEqual(entries);

    await expect(
      backend(
        () => Promise.resolve([{ entryId: 9, track: { ...track, album: 1 } }]),
        true,
      ).playlistEntries(1),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });

  it('sends settings and accepts only known values back', async () => {
    const settings = {
      dsdTransport: 'auto',
      dsdFallback: 'convert',
      conversionGainDb: 3,
      resyncDelayMs: 500,
      dopDevices: ['hw:CARD=E30,DEV=0'],
      onlineLookups: false,
    } satisfies Settings;

    const runCommand = vi.fn(() => Promise.resolve(settings));

    await expect(
      backend(runCommand, true).setSettings(settings),
    ).resolves.toEqual(settings);
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('set_settings', {
      settings,
    });

    await expect(
      backend(
        () => Promise.resolve({ ...settings, dsdFallback: 'resample' }),
        true,
      ).settings(),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });

  it('sends ID Match choices as keys only, never values', async () => {
    const runCommand = vi.fn(() => Promise.resolve(null));
    const target = { kind: 'track', id: 4 } as const;
    await backend(runCommand, true).applyMatch(
      7,
      'b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d',
      [{ track: 4, slot: { medium: 0, index: 2 } }],
      [{ target, field: 'title' }],
      [{ target, revision: 12 }],
      true,
    );
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('apply_match', {
      albumId: 7,
      releaseId: 'b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d',
      pairs: [{ track: 4, slot: { medium: 0, index: 2 } }],
      accepted: [{ target, field: 'title' }],
      revisions: [{ target, revision: 12 }],
      cover: true,
    });
  });

  it('accepts only inline cover previews', async () => {
    const cover = (value: JsonValue) =>
      backend(() => Promise.resolve(value), true).matchCover('x');

    await expect(cover('data:image/png;base64,iVBORw0KGgo=')).resolves.toBe(
      'data:image/png;base64,iVBORw0KGgo=',
    );
    await expect(cover(null)).resolves.toBeNull();

    for (const remote of [
      'https://coverartarchive.org/release/x/front',
      'data:text/html;base64,PHNjcmlwdD4=',
    ]) {
      await expect(cover(remote)).rejects.toMatchObject({
        code: 'invalid_response',
      });
    }
  });

  it('checks an edition comparison', async () => {
    const release = {
      releaseId: 'b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d',
      title: 'Kind of Blue',
      artist: 'Miles Davis',
      disambiguation: null,
      date: '1959-08-17',
      country: 'US',
      status: 'Official',
      labels: ['Columbia CL 1355'],
      formats: ['12" Vinyl'],
      trackCount: 1,
      evidence: {
        releaseId: false,
        trackCount: true,
        title: true,
        artist: true,
        year: true,
      },
    };

    const info = {
      release,
      url: 'https://musicbrainz.org/release/b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d',
      media: [
        {
          position: 1,
          format: 'CD',
          title: null,
          tracks: [{ number: '1', title: 'So What', seconds: 562 }],
        },
      ],
      tracks: [
        { id: 4, disc: null, number: '1', title: 'so what', seconds: 561.5 },
      ],
      pairs: [
        {
          track: 4,
          slot: { medium: 0, index: 0 },
          by: 'number',
          lengthDifferenceSeconds: -0.5,
          lengthsAgree: true,
        },
      ],
      unpaired: [],
      missing: [],
      suggestions: [
        {
          target: { kind: 'track', id: 4 },
          field: 'title',
          fieldLabel: 'Title',
          before: ['so what'],
          after: ['So What'],
          kept: false,
        },
      ],
      revisions: [{ target: { kind: 'release', id: 7 }, revision: 3 }],
    };

    const compare = (value: JsonValue) =>
      backend(() => Promise.resolve(value), true).matchRelease(7, 'x');

    await expect(compare(info)).resolves.toEqual(info);
    await expect(
      compare({ ...info, pairs: [{ ...info.pairs[0], by: 'guess' }] }),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });

  it('accepts a card volume, or none, and nothing malformed', async () => {
    const level = { percent: 63, decibels: -23.5, usb: false, locked: null };

    const volume = (value: JsonValue) =>
      backend(() => Promise.resolve(value), true).volume('hw:CARD=PCH,DEV=0');

    await expect(volume(level)).resolves.toEqual(level);
    await expect(volume(null)).resolves.toBeNull();
    await expect(volume({ ...level, usb: 'no' })).rejects.toMatchObject({
      code: 'invalid_response',
    });
  });

  it('accepts the album editor and nothing malformed', async () => {
    const field = {
      field: 'title',
      label: 'Title',
      values: ['Mysterons'],
      multiple: false,
      edited: false,
      review: false,
    };

    const editor = {
      albumId: 1,
      art: null,
      fields: [{ ...field, field: 'album', label: 'Album' }],
      tracks: [{ id: 2, fileName: '1.flac', fields: [field] }],
      revisions: [{ target: { kind: 'track', id: 2 }, revision: 7 }],
    };

    const load = (value: JsonValue) =>
      backend(() => Promise.resolve(value), true).albumEditor(1);

    await expect(load(editor)).resolves.toEqual(editor);
    await expect(
      load({ ...editor, revisions: [{ target: { kind: 'disc', id: 2 } }] }),
    ).rejects.toMatchObject({ code: 'invalid_response' });
    await expect(
      load({
        ...editor,
        tracks: [{ id: 2, fields: [{ ...field, values: 'x' }] }],
      }),
    ).rejects.toMatchObject({ code: 'invalid_response' });
  });

  it('sends edits with the revisions the editor saw', async () => {
    const runCommand = vi.fn(() => Promise.resolve(null));
    const target = { kind: 'track', id: 2 } as const;

    await backend(runCommand, true).saveAlbumEdits(
      [
        {
          target,
          field: 'genre',
          edit: { kind: 'set', values: ['Rock', 'Pop'] },
        },
      ],
      [{ target, revision: 7 }],
    );
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('save_album_edits', {
      changes: [
        {
          target,
          field: 'genre',
          edit: { kind: 'set', values: ['Rock', 'Pop'] },
        },
      ],
      revisions: [{ target, revision: 7 }],
    });
  });

  it('accepts fixes by their new values and rule names', async () => {
    const runCommand = vi.fn(() => Promise.resolve(null));
    const target = { kind: 'release', id: 1 } as const;

    await backend(runCommand, true).applyFixes([
      {
        target,
        field: 'album',
        fieldLabel: 'Album',
        item: 'DUMMY',
        before: ['DUMMY'],
        after: ['Dummy'],
        rules: [{ name: 'capitals', label: 'Capitals' }],
      },
    ]);
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('apply_fixes', {
      accepted: [
        { target, field: 'album', values: ['Dummy'], rules: ['capitals'] },
      ],
    });
  });
});

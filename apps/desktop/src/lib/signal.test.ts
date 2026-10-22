import { describe, expect, it } from 'vitest';

import type { Output, Track } from './ipc';
import { signalStages } from './signal';

const output: Output = {
  transport: 'pcm',
  format: 'S32_LE',
  sampleRate: 44100,
  channels: 2,
  hardwareVolume: 'attenuated',
  hardwareDb: -12.5,
};

const track: Track = {
  libraryId: 1,
  queueIndex: 0,
  fileName: 'a.flac',
  device: 'hw:CARD=SE,DEV=0',
  codec: 'flac',
  sampleRate: 44100,
  bitsPerSample: 16,
  dsdMultiple: null,
  channels: 2,
  durationSeconds: 10,
  output,
  processing: [{ kind: 'padded', fromBits: 16, toBits: 32 }],
  bitPerfect: null,
};

const texts = (t: Track) =>
  signalStages(t, 'iFi USB Audio SE').map((s) => `${s.title}: ${s.text}`);

describe('signal path', () => {
  it('states each stage from the engine report', () => {
    expect(texts(track)).toEqual([
      'Source: FLAC · 16-bit · 44.1 kHz · 2 channels',
      'Processing: 16-bit samples sent as 32-bit, low bits zero (lossless)',
      'Volume: No software volume; DAC volume −12.5 dB',
      'Output: PCM · 32-bit · 44.1 kHz · 2 channels',
      'Device: iFi USB Audio SE',
    ]);
  });

  it('names conversion and its gain, and DSD rates in MHz', () => {
    const dsd: Track = {
      ...track,
      codec: 'dsf',
      sampleRate: 2822400,
      bitsPerSample: 1,
      dsdMultiple: 64,
      output: { ...output, transport: 'converted', sampleRate: 88200 },
      processing: [{ kind: 'converted', gainDb: -3 }],
    };

    expect(texts(dsd).slice(0, 2)).toEqual([
      'Source: DSD64 · DSF · 2.8224 MHz · 2 channels',
      'Processing: Converted to PCM: low-pass and decimation, −3 dB gain (not bit-perfect)',
    ]);
  });

  it('waits for the device before saying more', () => {
    expect(texts({ ...track, output: null, processing: [] })).toEqual([
      'Source: FLAC · 16-bit · 44.1 kHz · 2 channels',
      'Output: Opening the device…',
    ]);
  });

  it('names native DSD and DoP by their DSD rate', () => {
    const dsd: Track = { ...track, codec: 'dsf', dsdMultiple: 128 };

    expect(
      texts({ ...dsd, output: { ...output, transport: 'native' } })[3],
    ).toBe('Output: DSD128 · Native · 2 channels');
    expect(texts({ ...dsd, output: { ...output, transport: 'dop' } })[3]).toBe(
      'Output: DSD128 · DoP · 2 channels',
    );
  });
});

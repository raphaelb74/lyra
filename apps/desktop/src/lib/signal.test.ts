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
      'Source: FLAC · 16-bit · 44.1 kHz',
      'Processing: 16-bit in 32-bit, lossless',
      'DAC volume: −12.5 dB',
      'Output: PCM · 32-bit · 44.1 kHz',
      'Device: iFi USB Audio SE',
    ]);
  });

  it('adds what levelling does to the DAC volume', () => {
    const levelled = (offsetDecibels: number | null) =>
      signalStages(track, 'SE', {
        offsetDecibels,
        basis: 'album',
        loudness: -13.5,
        deviceDecibels: -12.5,
        limited: false,
      })[2]?.text;

    expect(levelled(-4.5)).toBe('−12.5 dB · levelled −4.5 dB (album)');
    expect(levelled(3)).toBe('−12.5 dB · levelled +3.0 dB (album)');
    expect(levelled(null)).toBe('−12.5 dB · levelling: measuring');
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
      'Source: DSD64 · DSF · 2.8224 MHz',
      'Processing: DSD to PCM, −3 dB',
    ]);
  });

  it('names channels only when not stereo', () => {
    expect(texts({ ...track, channels: 1 })[0]).toBe(
      'Source: FLAC · 16-bit · 44.1 kHz · Mono',
    );
  });

  it('waits for the device before saying more', () => {
    expect(texts({ ...track, output: null, processing: [] })).toEqual([
      'Source: FLAC · 16-bit · 44.1 kHz',
      'Output: Opening…',
    ]);
  });

  it('names native DSD and DoP by their DSD rate', () => {
    const dsd: Track = { ...track, codec: 'dsf', dsdMultiple: 128 };

    expect(
      texts({ ...dsd, output: { ...output, transport: 'native' } })[3],
    ).toBe('Output: DSD128 · Native');
    expect(texts({ ...dsd, output: { ...output, transport: 'dop' } })[3]).toBe(
      'Output: DSD128 · DoP',
    );
  });
});

import { describe, expect, it } from 'vitest';

import {
  clock,
  formatBadge,
  outputLabel,
  playbackLabel,
  scanText,
  sourceLabel,
} from './format';
import type { Track } from './ipc';

const flac: Track = {
  libraryId: null,
  queueIndex: 0,
  fileName: 'a.flac',
  device: 'hw:CARD=ONE,DEV=0',
  codec: 'flac',
  sampleRate: 44100,
  bitsPerSample: 16,
  dsdMultiple: null,
  channels: 2,
  durationSeconds: null,
  output: null,
  bitPerfect: null,
};

describe('display formatting', () => {
  it('reports scan progress and results', () => {
    const idle = { running: false, done: 0, total: 0, last: null, error: null };

    expect(scanText({ ...idle, running: true })).toBe('Looking for music…');
    expect(scanText({ ...idle, running: true, done: 12, total: 340 })).toBe(
      'Scanning 12 of 340 files…',
    );
    expect(
      scanText({
        ...idle,
        last: { added: 3, updated: 1, removed: 0, failed: 2 },
      }),
    ).toBe('Up to date · 3 added, 1 updated, 0 removed · 2 could not be read');
  });

  it('names what reaches the DAC in plain words', () => {
    const dsd = {
      ...flac,
      codec: 'dsf' as const,
      bitsPerSample: 1,
      dsdMultiple: 128,
    };

    const wire = { format: 'S32_LE', channels: 2 };

    expect(playbackLabel(flac)).toBeNull();
    expect(
      playbackLabel({
        ...flac,
        bitsPerSample: 24,
        output: { ...wire, transport: 'pcm', sampleRate: 96000 },
      }),
    ).toBe('PCM · 24-bit / 96 kHz');
    expect(
      playbackLabel({
        ...dsd,
        output: { ...wire, transport: 'native', sampleRate: 176400 },
      }),
    ).toBe('DSD128 · Native');
    expect(
      playbackLabel({
        ...dsd,
        output: { ...wire, transport: 'dop', sampleRate: 352800 },
      }),
    ).toBe('DSD128 · DoP');
    expect(
      playbackLabel({
        ...dsd,
        dsdMultiple: 64,
        output: { ...wire, transport: 'converted', sampleRate: 88200 },
      }),
    ).toBe('DSD64 → PCM 88.2 kHz');
  });

  it('labels PCM source and output the way audiophiles read them', () => {
    expect(sourceLabel(flac)).toBe('FLAC 16/44.1');
    expect(
      outputLabel({
        transport: 'pcm',
        format: 'S24_3LE',
        sampleRate: 96000,
        channels: 2,
      }),
    ).toBe('S24_3LE · 96 kHz');
  });

  it('says when DSD was converted to PCM', () => {
    expect(
      outputLabel({
        transport: 'converted',
        format: 'S32_LE',
        sampleRate: 88200,
        channels: 2,
      }),
    ).toBe('DSD converted to PCM · S32_LE · 88.2 kHz');
  });

  it('labels DSD sources and DoP output', () => {
    expect(
      sourceLabel({
        ...flac,
        codec: 'dsf',
        sampleRate: 2822400,
        bitsPerSample: 1,
        dsdMultiple: 64,
      }),
    ).toBe('DSD64 · DSF');
    expect(
      outputLabel({
        transport: 'dop',
        format: 'S24_3LE',
        sampleRate: 176400,
        channels: 2,
      }),
    ).toBe('DoP · S24_3LE · 176.4 kHz');
    expect(
      outputLabel({
        transport: 'native',
        format: 'DSD_U32_BE',
        sampleRate: 88200,
        channels: 2,
      }),
    ).toBe('Native DSD · DSD_U32_BE · 88.2 kHz');
  });

  it('badges albums by their best format', () => {
    expect(
      formatBadge({ codec: 'dsf', sampleRate: 5644800, dsdMultiple: 128 }),
    ).toBe('DSD128');
    expect(
      formatBadge({ codec: 'flac', sampleRate: 96000, dsdMultiple: null }),
    ).toBe('FLAC 96');
  });

  it.each([
    [0, '0:00'],
    [7.9, '0:07'],
    [187, '3:07'],
    [3725, '1:02:05'],
    [-3, '0:00'],
  ])('formats %d s as %s', (seconds, text) => {
    expect(clock(seconds)).toBe(text);
  });
});

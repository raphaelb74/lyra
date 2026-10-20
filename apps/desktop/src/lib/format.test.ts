import { describe, expect, it } from 'vitest';

import { clock, outputLabel, sourceLabel } from './format';

describe('display formatting', () => {
  it('labels source and output the way audiophiles read them', () => {
    expect(
      sourceLabel({
        fileName: 'a.flac',
        device: 'hw:CARD=ONE,DEV=0',
        codec: 'flac',
        sampleRate: 44100,
        bitsPerSample: 16,
        channels: 2,
        durationSeconds: null,
        output: null,
        bitPerfect: null,
      }),
    ).toBe('FLAC 16/44.1');
    expect(
      outputLabel({ format: 'S24_3LE', sampleRate: 96000, channels: 2 }),
    ).toBe('S24_3LE · 96 kHz');
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

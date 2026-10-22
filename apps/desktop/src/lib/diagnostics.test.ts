import { describe, expect, it } from 'vitest';

import {
  conversionLabel,
  diagnosticsReport,
  framesLabel,
  supportLabel,
} from './diagnostics';
import type { DiagnosticsInfo } from './ipc';

const idle: DiagnosticsInfo = {
  device: 'hw:CARD=SE,DEV=0',
  kernel: '7.2.6-generic',
  alsa: null,
  usbId: '2fc6:f082',
  hwParams: null,
  usbStream: null,
  playback: null,
};

describe('diagnostics', () => {
  it('gives buffer sizes in frames and milliseconds', () => {
    expect(framesLabel(22050, 88200)).toBe('22050 frames (250 ms)');
    expect(framesLabel(5513, null)).toBe('5513 frames');
  });

  it('describes conversion level and clipping', () => {
    expect(conversionLabel({ clipped: 0, peakDb: -3.24 })).toBe(
      'Peak −3.2 dBFS, no samples clipped',
    );
    expect(conversionLabel({ clipped: 12, peakDb: 0.4 })).toBe(
      'Peak 0.4 dBFS, 12 samples clipped',
    );
    expect(conversionLabel({ clipped: 0, peakDb: null })).toBe(
      'Silence so far, no samples clipped',
    );
  });

  it('names accepted formats for people', () => {
    expect(supportLabel({ format: 'DSD_U32_BE', rates: [88200, 176400] })).toBe(
      'Native DSD: DSD64, DSD128',
    );
    expect(supportLabel({ format: 'DSD_U8', rates: [352800] })).toBe(
      'Native DSD: DSD64',
    );
    expect(supportLabel({ format: 'S32_LE', rates: [44100, 384000] })).toBe(
      'PCM 32-bit: 44.1, 384 kHz',
    );
    expect(supportLabel({ format: 'S24_3LE', rates: [96000] })).toBe(
      'PCM 24-bit: 96 kHz',
    );
    expect(supportLabel({ format: 'S24_LE', rates: [96000] })).toBe(
      'PCM 24-bit in 32-bit: 96 kHz',
    );
  });

  it('reports an idle device without playback lines', () => {
    const report = diagnosticsReport(undefined, idle, null, null);

    expect(report).toBe(
      [
        'Lyra diagnostics for hw:CARD=SE,DEV=0',
        'Kernel: 7.2.6-generic',
        'USB ID: 2fc6:f082',
        '',
        'Lyra is not playing on this device.',
      ].join('\n'),
    );
  });

  it('reports playback, the kernel view and capabilities', () => {
    const report = diagnosticsReport(
      {
        name: 'hw:CARD=SE,DEV=0',
        label: 'iFi USB Audio SE',
        cardName: 'iFi USB Audio SE',
        pcmName: 'USB Audio',
        usb: true,
      },
      {
        ...idle,
        hwParams: 'format: DSD_U32_BE\nrate: 88200 (88200/1)',
        usbStream: 'Altset = 4\nMomentary freq = 88203 Hz',
        playback: {
          underruns: 0,
          bufferFrames: 22050,
          periodFrames: 5513,
          takenFrom: 'pipewire',
          conversion: null,
        },
      },
      {
        transport: 'native',
        format: 'DSD_U32_BE',
        sampleRate: 88200,
        channels: 2,
        hardwareVolume: 'none',
        hardwareDb: null,
      },
      {
        channelsMin: 2,
        channelsMax: 2,
        formats: [{ format: 'S16_LE', rates: [44100, 48000] }],
      },
    );

    expect(report).toContain(
      'Lyra diagnostics for iFi USB Audio SE, USB Audio (hw:CARD=SE,DEV=0)',
    );
    expect(report).toContain('Output: Native DSD · DSD_U32_BE · 88.2 kHz');
    expect(report).toContain(
      'Buffer: 22050 frames (250 ms), period 5513 frames (63 ms)',
    );
    expect(report).toContain('Handed over by: pipewire');
    expect(report).toContain('USB stream:\nAltset = 4');
    expect(report).toContain(
      'Accepts (2–2 channels):\nPCM 16-bit: 44.1, 48 kHz (S16_LE)',
    );
  });
});

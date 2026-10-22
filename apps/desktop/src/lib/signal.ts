import { wireLabel } from './format';
import type { Output, Step, Track } from './ipc';

/** One stage of the signal path, from the engine's report only. */
export interface Stage {
  title: 'Source' | 'Processing' | 'Volume' | 'Output' | 'Device';
  text: string;
  /** Muted second line. */
  detail: string | null;
}

/** `96 kHz`, `44.1 kHz`, `2.8224 MHz`. */
function rate(hertz: number): string {
  return hertz >= 1_000_000
    ? `${String(hertz / 1_000_000)} MHz`
    : `${String(hertz / 1000)} kHz`;
}

function channels(count: number): string {
  return count === 1 ? '1 channel' : `${String(count)} channels`;
}

function stepText(step: Step): string {
  switch (step.kind) {
    case 'padded':
      return `${String(step.fromBits)}-bit samples sent as ${String(step.toBits)}-bit, low bits zero (lossless)`;
    case 'dopPacked':
      return 'Packed into DoP markers (lossless for the DAC)';
    case 'converted': {
      const gain =
        step.gainDb === 0
          ? 'no gain'
          : `${step.gainDb > 0 ? '+' : '−'}${String(Math.abs(step.gainDb))} dB gain`;

      return `Converted to PCM: low-pass and decimation, ${gain} (not bit-perfect)`;
    }
  }
}

function volumeText(output: Output): string {
  switch (output.hardwareVolume) {
    case 'none':
      return 'No software volume; the DAC has no volume control';
    case 'unity':
      return 'No software volume; DAC volume at 0 dB';
    case 'attenuated':
      return `No software volume; DAC volume ${decibels(output.hardwareDb ?? 0)}`;
    case 'unknown':
      return 'No software volume; the DAC volume level cannot be read';
  }
}

/** `−12.5 dB`. */
function decibels(value: number): string {
  return `${value < 0 ? '−' : ''}${String(Math.abs(value))} dB`;
}

/** Source, processing, volume, output and device, in that order. */
export function signalStages(track: Track, deviceLabel: string): Stage[] {
  const source =
    track.dsdMultiple === null
      ? `${track.codec.toUpperCase()} · ${String(track.bitsPerSample)}-bit · ${rate(track.sampleRate)}`
      : `DSD${String(track.dsdMultiple)} · ${track.codec.toUpperCase()} · ${rate(track.sampleRate)}`;

  const stages: Stage[] = [
    {
      title: 'Source',
      text: `${source} · ${channels(track.channels)}`,
      detail: track.fileName,
    },
  ];

  const output = track.output;

  if (output === null) {
    stages.push({ title: 'Output', text: 'Opening the device…', detail: null });

    return stages;
  }

  stages.push(
    {
      title: 'Processing',
      text:
        track.processing.length === 0
          ? 'None: samples sent as read'
          : track.processing.map(stepText).join('; '),
      detail: null,
    },
    { title: 'Volume', text: volumeText(output), detail: null },
    {
      title: 'Output',
      text: `${wireLabel(track, output)} · ${channels(output.channels)}`,
      detail: null,
    },
    {
      title: 'Device',
      text: deviceLabel,
      detail: null,
    },
  );

  return stages;
}

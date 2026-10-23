import { decibels, levelled, wireLabel } from './format';
import type { LevellingInfo, Output, Step, Track } from './ipc';

/** One stage of the signal path, from the engine's report only. */
export interface Stage {
  title: 'Source' | 'Processing' | 'DAC volume' | 'Output' | 'Device';
  text: string;
}

/** `96 kHz`, `44.1 kHz`, `2.8224 MHz`. */
function rate(hertz: number): string {
  return hertz >= 1_000_000
    ? `${String(hertz / 1_000_000)} MHz`
    : `${String(hertz / 1000)} kHz`;
}

/** Said only when unusual: stereo goes without saying. */
function channels(count: number): string {
  if (count === 2) return '';

  return count === 1 ? ' · Mono' : ` · ${String(count)} ch`;
}

function stepText(step: Step): string {
  switch (step.kind) {
    case 'padded':
      return `${String(step.fromBits)}-bit in ${String(step.toBits)}-bit, lossless`;
    case 'dopPacked':
      return 'DoP packing, lossless';
    case 'converted':
      return step.gainDb === 0
        ? 'DSD to PCM'
        : `DSD to PCM, ${step.gainDb > 0 ? '+' : '−'}${String(Math.abs(step.gainDb))} dB`;
  }
}

function volumeText(output: Output): string {
  switch (output.hardwareVolume) {
    case 'none':
      return 'Fixed';
    case 'unity':
      return '0 dB';
    case 'attenuated':
      return decibels(output.hardwareDb ?? 0);
    case 'unknown':
      return 'Unknown';
  }
}

/** `· levelled −4.5 dB (album)` after the DAC volume, while levelling. */
function levellingText(levelling: LevellingInfo | null): string {
  return levelling === null ? '' : ` · ${levelled(levelling)}`;
}

/** Source, processing, DAC volume, output and device, in that order. */
export function signalStages(
  track: Track,
  deviceLabel: string,
  levelling: LevellingInfo | null = null,
): Stage[] {
  const source =
    track.dsdMultiple === null
      ? `${track.codec.toUpperCase()} · ${String(track.bitsPerSample)}-bit · ${rate(track.sampleRate)}`
      : `DSD${String(track.dsdMultiple)} · ${track.codec.toUpperCase()} · ${rate(track.sampleRate)}`;

  const stages: Stage[] = [
    { title: 'Source', text: `${source}${channels(track.channels)}` },
  ];

  const output = track.output;

  if (output === null) {
    stages.push({ title: 'Output', text: 'Opening…' });

    return stages;
  }

  stages.push(
    {
      title: 'Processing',
      text:
        track.processing.length === 0
          ? 'None'
          : track.processing.map(stepText).join(' · '),
    },
    {
      title: 'DAC volume',
      text: `${volumeText(output)}${levellingText(levelling)}`,
    },
    {
      title: 'Output',
      text: `${wireLabel(track, output)}${channels(output.channels)}`,
    },
    { title: 'Device', text: deviceLabel },
  );

  return stages;
}

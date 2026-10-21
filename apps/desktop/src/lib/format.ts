import type { Output, Track } from './ipc';

/** `FLAC 24/96`, `WAV 16/44.1`, `DSD64 · DSF`. */
export function sourceLabel(track: Track): string {
  if (track.dsdMultiple !== null) {
    return `DSD${String(track.dsdMultiple)} · ${track.codec.toUpperCase()}`;
  }

  return `${track.codec.toUpperCase()} ${String(track.bitsPerSample)}/${kilohertz(track.sampleRate)}`;
}

/** `S24_3LE · 96 kHz`, `DoP · S24_3LE · 176.4 kHz`, `Native DSD · DSD_U32_BE · 88.2 kHz`. */
export function outputLabel(output: Output): string {
  const wire = `${output.format} · ${kilohertz(output.sampleRate)} kHz`;

  switch (output.transport) {
    case 'pcm':
      return wire;
    case 'dop':
      return `DoP · ${wire}`;
    case 'native':
      return `Native DSD · ${wire}`;
  }
}

/** `44.1`, `96`, `352.8`. */
function kilohertz(rate: number): string {
  return String(rate / 1000);
}

/** `3:07`; hours when needed. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const secs = String(whole % 60).padStart(2, '0');

  if (hours > 0) {
    return `${String(hours)}:${String(minutes).padStart(2, '0')}:${secs}`;
  }

  return `${String(minutes)}:${secs}`;
}

import type { Output, Track } from './ipc';

/** `FLAC 24/96`, `WAV 16/44.1`. */
export function sourceLabel(track: Track): string {
  return `${track.codec.toUpperCase()} ${String(track.bitsPerSample)}/${kilohertz(track.sampleRate)}`;
}

/** `S24_3LE · 96 kHz`. */
export function outputLabel(output: Output): string {
  return `${output.format} · ${kilohertz(output.sampleRate)} kHz`;
}

/** `44.1`, `96`, `352.8`. */
export function kilohertz(rate: number): string {
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

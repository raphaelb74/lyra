import type { Output, ScanStatus, SourceFormat, Track, Verdict } from './ipc';

/** `FLAC 24/96`, `WAV 16/44.1`, `DSD64 · DSF`. */
export function sourceLabel(track: Track): string {
  if (track.dsdMultiple !== null) {
    return `DSD${String(track.dsdMultiple)} · ${track.codec.toUpperCase()}`;
  }

  return `${track.codec.toUpperCase()} ${String(track.bitsPerSample)}/${kilohertz(track.sampleRate)}`;
}

/** Short badge without bit depth: `DSD128`, `FLAC 96`. */
export function formatBadge(format: SourceFormat): string {
  if (format.dsdMultiple !== null) return `DSD${String(format.dsdMultiple)}`;

  return `${format.codec.toUpperCase()} ${kilohertz(format.sampleRate)}`;
}

/** A track's source format with its bit depth (1 for DSD). */
type TrackFormat = SourceFormat & { bitsPerSample: number };

/** `DSD64`, or `FLAC 24/96` with the bit depth. */
export function formatLabel(format: TrackFormat): string {
  if (format.dsdMultiple !== null) return `DSD${String(format.dsdMultiple)}`;

  return `${format.codec.toUpperCase()} ${String(format.bitsPerSample)}/${kilohertz(format.sampleRate)}`;
}

/** Above CD quality: DSD, more than 16 bits or more than 48 kHz. */
export function isHiRes(format: TrackFormat): boolean {
  return (
    format.dsdMultiple !== null ||
    format.bitsPerSample > 16 ||
    format.sampleRate > 48000
  );
}

/** Each format of an album's tracks once, highest rate first. */
export function albumFormats(tracks: readonly TrackFormat[]): string[] {
  const sorted = [...tracks].sort(
    (a, b) =>
      b.sampleRate * Math.max(1, b.bitsPerSample) -
      a.sampleRate * Math.max(1, a.bitsPerSample),
  );

  return [...new Set(sorted.map(formatLabel))];
}

/** Album length: `42 min`, `1 h 12 min`. */
export function albumLength(seconds: number): string {
  const minutes = Math.round(seconds / 60);

  if (minutes < 60) return `${String(minutes)} min`;

  return `${String(Math.floor(minutes / 60))} h ${String(minutes % 60)} min`;
}

/**
 * `S24_3LE · 96 kHz`, `DoP · S24_3LE · 176.4 kHz`,
 * `Native DSD · DSD_U32_BE · 88.2 kHz`, `DSD converted to PCM · S32_LE · 88.2 kHz`.
 */
export function outputLabel(output: Output): string {
  const wire = `${output.format} · ${kilohertz(output.sampleRate)} kHz`;

  switch (output.transport) {
    case 'pcm':
      return wire;
    case 'dop':
      return `DoP · ${wire}`;
    case 'native':
      return `Native DSD · ${wire}`;
    case 'converted':
      return `DSD converted to PCM · ${wire}`;
  }
}

/**
 * What reaches the DAC, for people: `PCM · 24-bit / 96 kHz`, `DSD128 · Native`,
 * `DSD128 · DoP`, `DSD64 → PCM 88.2 kHz`. From the engine's output, never from
 * file metadata alone; null until the device is open.
 */
export function playbackLabel(track: Track): string | null {
  const output = track.output;

  if (output === null) return null;

  const dsd = `DSD${String(track.dsdMultiple ?? '')}`;

  switch (output.transport) {
    case 'pcm':
      return `PCM · ${String(track.bitsPerSample)}-bit / ${kilohertz(output.sampleRate)} kHz`;
    case 'native':
      return `${dsd} · Native`;
    case 'dop':
      return `${dsd} · DoP`;
    case 'converted':
      return `${dsd} → PCM ${kilohertz(output.sampleRate)} kHz`;
  }
}

/**
 * The colour of what reaches the DAC, after iFi's format LED: `cd` (yellow,
 * PCM up to 48 kHz), `hiRes` (white, PCM above), `dsd` (cyan, DSD64 and
 * DSD128) and `dsdHigh` (red, DSD256 and up). Null until the device is open.
 */
export type FormatTone = 'cd' | 'hiRes' | 'dsd' | 'dsdHigh';

export function formatTone(track: Track): FormatTone | null {
  const output = track.output;

  if (output === null) return null;

  switch (output.transport) {
    case 'pcm':
    case 'converted':
      return output.sampleRate <= 48000 ? 'cd' : 'hiRes';
    case 'native':
    case 'dop':
      return (track.dsdMultiple ?? 0) >= 256 ? 'dsdHigh' : 'dsd';
  }
}

/** Bits per sample of an ALSA PCM format (`S24_3LE` → 24); null otherwise. */
export function pcmBits(format: string): number | null {
  const bits = /^[SU](\d+)_/.exec(format)?.[1];

  if (bits !== undefined) return Number(bits);

  return format.startsWith('FLOAT_') ? 32 : null;
}

/**
 * What the DAC receives, for people: `PCM · 32-bit · 96 kHz`,
 * `DSD128 · Native`, `DSD64 · DoP`.
 */
export function wireLabel(track: Track, output: Output): string {
  const dsd = `DSD${String(track.dsdMultiple ?? '')}`;

  switch (output.transport) {
    case 'pcm':
    case 'converted': {
      const bits = pcmBits(output.format);
      const depth = bits === null ? output.format : `${String(bits)}-bit`;

      return `PCM · ${depth} · ${kilohertz(output.sampleRate)} kHz`;
    }

    case 'native':
      return `${dsd} · Native`;
    case 'dop':
      return `${dsd} · DoP`;
  }
}

/** `−12.5 dB`, `0.0 dB`; `unit` for `dBFS`. */
export function decibels(value: number, unit = 'dB'): string {
  return `${value.toFixed(1).replace('-', '−')} ${unit}`;
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

/** `Bit-perfect`, `Not bit-perfect` or `Unverified`; the reason is apart. */
export function verdictLabel(verdict: Verdict | null): string {
  if (verdict === null || verdict.status === 'unverified') return 'Unverified';

  return verdict.status === 'yes' ? 'Bit-perfect' : 'Not bit-perfect';
}

/** `Scanning 12 of 340 files…`, `Up to date · 3 added, …`, or the error. */
export function scanText(scan: ScanStatus): string {
  if (scan.running) {
    return scan.total === 0
      ? 'Looking for music…'
      : `Scanning ${String(scan.done)} of ${String(scan.total)} files…`;
  }

  if (scan.error !== null) return scan.error;

  const last = scan.last;

  if (last === null) return '';

  const failed =
    last.failed === 0 ? '' : ` · ${String(last.failed)} could not be read`;

  const missing = last.unavailable.length;

  const unavailable =
    missing === 0
      ? ''
      : missing === 1
        ? ' · 1 folder not found, its tracks kept'
        : ` · ${String(missing)} folders not found, their tracks kept`;

  return `Up to date · ${String(last.added)} added, ${String(last.updated)} updated, ${String(last.removed)} removed${failed}${unavailable}`;
}

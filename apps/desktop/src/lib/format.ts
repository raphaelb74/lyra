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

/** `Bit-perfect`, `Not bit-perfect: <reason>`, … */
export function verdictLabel(verdict: Verdict | null): string {
  if (verdict === null) return 'Bit-perfect: not playing';

  if (verdict.status === 'yes') return 'Bit-perfect';

  const reason = verdict.reason ?? 'unknown';

  return verdict.status === 'no'
    ? `Not bit-perfect: ${reason}`
    : `Bit-perfect unverified: ${reason}`;
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

  return `Up to date · ${String(last.added)} added, ${String(last.updated)} updated, ${String(last.removed)} removed${failed}`;
}

import { outputLabel } from './format';
import type {
  Capabilities,
  ConversionInfo,
  DeviceInfo,
  DiagnosticsInfo,
  Output,
  PlaybackDiagnostics,
} from './ipc';

/** `22050 frames (250 ms)`; frames only without a rate. */
export function framesLabel(frames: number, rate: number | null): string {
  const text = `${String(frames)} frames`;

  if (rate === null || rate <= 0) return text;

  return `${text} (${String(Math.round((frames / rate) * 1000))} ms)`;
}

/** `Peak −3.2 dBFS, no samples clipped`. */
export function conversionLabel(conversion: ConversionInfo): string {
  const peak =
    conversion.peakDb === null
      ? 'Silence so far'
      : `Peak ${conversion.peakDb.toFixed(1).replace('-', '−')} dBFS`;

  const clipped =
    conversion.clipped === 0
      ? 'no samples clipped'
      : `${String(conversion.clipped)} samples clipped`;

  return `${peak}, ${clipped}`;
}

/** `S24_3LE: 44.1, 48, 96 kHz`. */
export function supportLabel(support: Capabilities['formats'][number]): string {
  const rates = support.rates.map((rate) => String(rate / 1000)).join(', ');

  return `${support.format}: ${rates} kHz`;
}

function playbackLines(
  playback: PlaybackDiagnostics,
  output: Output | null,
): string[] {
  const rate = output?.sampleRate ?? null;

  const lines = [
    `Output: ${output === null ? 'opening' : outputLabel(output)}`,
    `Underruns since the device opened: ${String(playback.underruns)}`,
    `Buffer: ${framesLabel(playback.bufferFrames, rate)}, period ${framesLabel(playback.periodFrames, rate)}`,
  ];

  if (playback.takenFrom !== null) {
    lines.push(`Handed over by: ${playback.takenFrom}`);
  }

  if (playback.conversion !== null) {
    lines.push(`Conversion: ${conversionLabel(playback.conversion)}`);
  }

  return lines;
}

/** Plain text for a bug report or a `docs/hardware/` file. */
export function diagnosticsReport(
  device: DeviceInfo | undefined,
  info: DiagnosticsInfo,
  output: Output | null,
  capabilities: Capabilities | null,
): string {
  const name =
    device === undefined
      ? info.device
      : `${device.cardName}, ${device.pcmName} (${info.device})`;

  const lines = [`Lyra diagnostics for ${name}`];

  const facts: [string, string | null][] = [
    ['Kernel', info.kernel],
    ['ALSA', info.alsa],
    ['USB ID', info.usbId],
  ];

  for (const [label, value] of facts) {
    if (value !== null) lines.push(`${label}: ${value}`);
  }

  lines.push('');

  if (info.playback === null) {
    lines.push('Lyra is not playing on this device.');
  } else {
    lines.push(...playbackLines(info.playback, output));
  }

  const blocks: [string, string | null][] = [
    ['Kernel hw_params', info.hwParams],
    ['USB stream', info.usbStream],
  ];

  for (const [label, value] of blocks) {
    if (value !== null) lines.push('', `${label}:`, value);
  }

  if (capabilities !== null) {
    lines.push(
      '',
      `Accepts (${String(capabilities.channelsMin)}–${String(capabilities.channelsMax)} channels):`,
      ...capabilities.formats.map(supportLabel),
    );
  }

  return lines.join('\n');
}

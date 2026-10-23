import type { VolumeInfo } from './ipc';

/** Lowest point of a dB slider: below it, listening level is near silence
 * on any DAC, and the useful range gets squeezed into a few pixels. */
export const FLOOR_DB = -60;

/** How the slider maps onto the device's volume. */
export interface VolumeScale {
  unit: 'db' | 'percent';
  min: number;
  max: number;
  /** Arrow keys and the mouse wheel. */
  step: number;
  /** Ctrl+↑ / Ctrl+↓. */
  jump: number;
  /** Where the handle sits now, within min…max. */
  value: number;
}

/** In dB from max(−60 dB, the device floor) to 0 dB when every control
 * reports dB; otherwise 0–100 % along the control's steps. */
export function volumeScale(level: VolumeInfo): VolumeScale {
  const { decibels, minDecibels, maxDecibels } = level;

  if (decibels === null || minDecibels === null || maxDecibels === null) {
    return {
      unit: 'percent',
      min: 0,
      max: 100,
      step: 1,
      jump: 5,
      value: level.percent,
    };
  }

  const max = Math.min(0, maxDecibels);
  const min = Math.min(max, Math.max(FLOOR_DB, minDecibels));

  return {
    unit: 'db',
    min,
    max,
    step: 0.5,
    jump: 2,
    value: Math.min(max, Math.max(min, decibels)),
  };
}

/** `value` within the scale, on its step. */
export function snap(scale: VolumeScale, value: number): number {
  const stepped = Math.round(value / scale.step) * scale.step;

  return Math.min(scale.max, Math.max(scale.min, stepped));
}

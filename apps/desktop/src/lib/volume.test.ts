import { describe, expect, it } from 'vitest';

import type { VolumeInfo } from './ipc';
import { snap, volumeScale } from './volume';

const usb: VolumeInfo = {
  percent: 80,
  decibels: -25.5,
  minDecibels: -127,
  maxDecibels: 0,
  usb: true,
  locked: null,
  muted: false,
  capDecibels: 0,
  levelling: null,
};

describe('volume scale', () => {
  it('runs from −60 dB to 0 dB on a DAC with a deeper range', () => {
    expect(volumeScale(usb)).toMatchObject({
      unit: 'db',
      min: -60,
      max: 0,
      step: 0.5,
      value: -25.5,
    });
  });

  it('keeps a shallower device floor and no gain above 0 dB', () => {
    const scale = volumeScale({ ...usb, minDecibels: -40, maxDecibels: 6 });

    expect([scale.min, scale.max]).toEqual([-40, 0]);
  });

  it('stops at the device cap', () => {
    const scale = volumeScale({ ...usb, decibels: -10, capDecibels: -20 });

    expect([scale.max, scale.value]).toEqual([-20, -20]);
  });

  it('parks the handle at the floor when the device is quieter', () => {
    expect(volumeScale({ ...usb, decibels: -90 }).value).toBe(-60);
  });

  it('falls back to percent without dB information', () => {
    expect(
      volumeScale({ ...usb, decibels: null, minDecibels: null }),
    ).toMatchObject({ unit: 'percent', min: 0, max: 100, value: 80 });
  });

  it('snaps to the step and stays in range', () => {
    const scale = volumeScale(usb);

    expect(snap(scale, -12.3)).toBe(-12.5);
    expect(snap(scale, 3)).toBe(0);
    expect(snap(scale, -80)).toBe(-60);
  });
});

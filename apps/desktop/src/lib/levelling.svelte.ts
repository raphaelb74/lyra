/**
 * What volume levelling does on each device, as its volume control last
 * heard from Rust; the signal path shows it beside the DAC volume.
 */

import type { LevellingInfo } from './ipc';

export const levelling = $state<Record<string, LevellingInfo | null>>({});

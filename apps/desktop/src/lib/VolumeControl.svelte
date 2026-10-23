<script lang="ts">
  import { onMount } from 'svelte';

  import {
    attempt,
    type Backend,
    type PlayerStatus,
    type VolumeInfo,
  } from './ipc';
  import { onDeviceVolume } from './events';
  import Icon from './Icon.svelte';
  import type { IconName } from '../assets/icons/lucide';
  import Popover from './Popover.svelte';
  import Slider from './Slider.svelte';
  import { decibels } from './format';
  import { snap, volumeScale, type VolumeScale } from './volume';

  interface Props {
    api: Backend;
    /** ALSA `hw:` device whose card volume this controls. */
    device: string;
    /** Reloads when playback changes (the DoP lock follows it). */
    status: PlayerStatus;
    onError: (message: string) => void;
  }

  const { api, device, status, onError }: Props = $props();

  let volume = $state<VolumeInfo | null>(null);
  /** Latest value asked for while a change is on its way. */
  let pending: number | null = null;
  /** Last value asked for until the device has it; steps add to it. */
  let target: number | null = null;
  let sending = false;

  /** Changes when playback starts, stops or changes transport: the DoP
   * lock depends on it. */
  const playback = $derived(
    `${status.state}:${status.track?.output?.transport ?? 'none'}`,
  );
  const notBitPerfect = $derived(
    volume !== null &&
      volume.usb &&
      volume.decibels !== null &&
      volume.decibels < 0,
  );

  /** Loads for `forDevice` in `forPlayback`; a stale reply is dropped. */
  async function load(forDevice: string, forPlayback: string) {
    const level = await attempt(() => api.volume(forDevice), onError);

    if (forDevice !== device || forPlayback !== playback) return;

    volume = level ?? null;
  }

  /** Sends `value` on `scale`, keeping one change in flight and only the
   * latest queued. */
  async function change(scale: VolumeScale, value: number) {
    pending = snap(scale, value);
    target = pending;

    if (sending) return;

    sending = true;

    while (pending !== null) {
      const next = pending;

      pending = null;

      const level = await attempt(
        () =>
          scale.unit === 'db'
            ? api.setVolumeDb(device, next)
            : api.setVolume(device, next),
        onError,
      );

      if (level !== undefined) volume = level;
    }

    sending = false;
    target = null;
  }

  /** Moves from the last value asked for, so quick steps add up. */
  function nudge(level: VolumeInfo, by: (scale: VolumeScale) => number) {
    const scale = volumeScale(level);

    void change(scale, (target ?? scale.value) + by(scale));
  }

  function onKey(event: KeyboardEvent) {
    if (!event.ctrlKey || volume?.locked !== null) return;

    const direction =
      event.key === 'ArrowUp' ? 1 : event.key === 'ArrowDown' ? -1 : 0;

    if (direction === 0) return;

    event.preventDefault();
    nudge(volume, (scale) => direction * scale.jump);
  }

  /** `−12.5 dB` where the card reports decibels, else `64 %`. */
  function valueText(level: VolumeInfo): string {
    return level.decibels === null
      ? `${String(level.percent)} %`
      : decibels(level.decibels);
  }

  /** Said only when something is off; shortcuts live in Settings › Keyboard. */
  function hint(level: VolumeInfo): string | null {
    if (level.locked !== null) return level.locked;

    return notBitPerfect ? 'Not bit-perfect below 0 dB' : null;
  }

  function icon(level: VolumeInfo): IconName {
    if (level.locked !== null) return 'lock';

    return level.percent === 0 ? 'volume-x' : 'volume-2';
  }

  /** The wheel over the button or the slider: one step, up is louder. */
  function onWheel(event: WheelEvent, level: VolumeInfo) {
    if (level.locked !== null || event.deltaY === 0) return;

    nudge(level, (scale) => (event.deltaY < 0 ? scale.step : -scale.step));
  }

  /** Follows changes made outside Lyra; ignored while a change is sent. */
  function followed(changed: string, level: VolumeInfo | null) {
    if (changed !== device || sending) return;

    volume = level;
  }

  onMount(() => {
    const unlisten = onDeviceVolume(followed);

    return () => {
      void unlisten.then((stop) => {
        stop();
      });
    };
  });

  $effect(() => {
    void load(device, playback);
  });
</script>

<svelte:window onkeydown={onKey} />

{#if volume}
  {@const level = volume}
  {@const scale = volumeScale(volume)}
  {@const text = valueText(volume)}
  {@const note = hint(volume)}
  <div
    class="volume"
    class:warn={notBitPerfect}
    onwheel={(event) => {
      onWheel(event, level);
    }}
  >
    <Popover
      label={`Volume: ${text}`}
      triggerClass="icon volume-trigger"
      above
      align="center"
      clear="footer"
    >
      {#snippet trigger()}
        <Icon name={icon(level)} size={18} />
      {/snippet}
      <p class="value num">{text}</p>
      <div class="track">
        <Slider
          label="Volume"
          value={scale.value}
          min={scale.min}
          max={scale.max}
          step={scale.step}
          valueText={text}
          disabled={level.locked !== null}
          onInput={(value: number) => void change(scale, value)}
        />
      </div>
      {#if note}<p class="hint">{note}</p>{/if}
    </Popover>
  </div>
{/if}

<style>
  .volume {
    display: flex;
  }
  .volume :global(.volume-trigger) {
    position: relative;
  }
  /* Below 0 dB on USB: an amber dot on the button, as on toggles. */
  .warn :global(.volume-trigger::after) {
    content: '';
    position: absolute;
    top: var(--space-1);
    right: var(--space-1);
    width: 6px;
    height: 6px;
    border-radius: var(--radius-round);
    background: var(--warning);
  }
  /* A narrow column. */
  .volume :global(div.panel) {
    width: 152px;
    min-width: 0;
    max-height: none;
    display: grid;
    justify-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
  }
  .value {
    font-size: var(--type-small);
    line-height: var(--leading-small);
    font-weight: var(--weight-medium);
  }
  .warn .value {
    color: var(--warning);
  }
  /* The range input turned upright: up is louder, and a tall track gives
     about 1 % per 1.6 px. Arrow keys step by 1 %. */
  .track {
    position: relative;
    width: var(--hit);
    height: 160px;
  }
  .track :global(input.slider) {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 160px;
    margin: 0;
    translate: -50% -50%;
    rotate: -90deg;
  }
  .hint {
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    color: var(--text-muted);
    text-align: center;
  }
</style>

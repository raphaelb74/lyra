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
  import Slider from './Slider.svelte';

  interface Props {
    api: Backend;
    /** ALSA `hw:` device whose card volume this controls. */
    device: string;
    /** Reloads when playback changes (the DoP lock follows it). */
    status: PlayerStatus;
    onError: (message: string) => void;
  }

  const { api, device, status, onError }: Props = $props();

  /** Ctrl+↑ / Ctrl+↓ step. */
  const STEP = 5;

  let volume = $state<VolumeInfo | null>(null);
  /** Latest value asked for while a change is on its way. */
  let pending: number | null = null;
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

  /** Sends `percent`, keeping one change in flight and only the latest queued. */
  async function change(percent: number) {
    pending = Math.min(100, Math.max(0, Math.round(percent)));

    if (sending) return;

    sending = true;

    while (pending !== null) {
      const next = pending;

      pending = null;

      const level = await attempt(() => api.setVolume(device, next), onError);

      if (level !== undefined) volume = level;
    }

    sending = false;
  }

  function onKey(event: KeyboardEvent) {
    if (!event.ctrlKey || volume?.locked !== null) return;

    const step =
      event.key === 'ArrowUp' ? STEP : event.key === 'ArrowDown' ? -STEP : 0;

    if (step === 0) return;

    event.preventDefault();
    void change(volume.percent + step);
  }

  /** `−12.5 dB` where the card reports decibels, else `64 %`. */
  function valueText(level: VolumeInfo): string {
    if (level.decibels === null) return `${String(level.percent)} %`;

    return `${level.decibels.toFixed(1).replace('-', '−')} dB`;
  }

  function hint(level: VolumeInfo): string {
    if (level.locked !== null) return level.locked;

    if (notBitPerfect) {
      return 'Below 0 dB, most USB DACs change the samples: not bit-perfect.';
    }

    return 'Volume of the device itself · Ctrl+↑ / Ctrl+↓';
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
  <div class="volume" title={hint(volume)}>
    <Icon
      name={volume.locked !== null
        ? 'lock'
        : volume.percent === 0
          ? 'volume-x'
          : 'volume-2'}
      size={16}
    />
    <Slider
      label="Volume"
      value={volume.percent}
      max={100}
      valueText={valueText(volume)}
      disabled={volume.locked !== null}
      onInput={(percent: number) => void change(percent)}
    />
    <span class="value num" class:warn={notBitPerfect}>{valueText(volume)}</span
    >
  </div>
{/if}

<style>
  .volume {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 1 168px;
    min-width: 80px;
    color: var(--text-secondary);
  }
  /* Floats above the slider's end; takes no room in the bar. */
  .value {
    position: absolute;
    right: 0;
    bottom: calc(100% - var(--space-1));
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    text-align: right;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .volume:hover .value,
  .volume:focus-within .value,
  .value.warn {
    opacity: 1;
  }
  .value.warn {
    color: var(--warning);
  }
</style>

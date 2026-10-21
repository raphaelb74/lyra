<script lang="ts">
  import { onMount } from 'svelte';

  import {
    attempt,
    type Backend,
    type PlayerStatus,
    type VolumeInfo,
  } from './ipc';
  import { onDeviceVolume } from './events';

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
  let loaded = $state(false);
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
    loaded = true;
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

  function onInput(event: Event) {
    if (event.target instanceof HTMLInputElement) {
      void change(Number(event.target.value));
    }
  }

  function onKey(event: KeyboardEvent) {
    if (!event.ctrlKey || volume?.locked !== null) return;

    const step =
      event.key === 'ArrowUp' ? STEP : event.key === 'ArrowDown' ? -STEP : 0;

    if (step === 0) return;

    event.preventDefault();
    void change(volume.percent + step);
  }

  function decibelText(level: VolumeInfo): string {
    if (level.decibels === null) return '';

    const value = level.decibels.toFixed(1).replace('-', '−');

    return ` · ${value} dB`;
  }

  /** Follows changes made outside Lyra; ignored while a change is sent. */
  function followed(changed: string, level: VolumeInfo | null) {
    if (changed !== device || sending) return;

    volume = level;
    loaded = true;
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
    loaded = false;
    void load(device, playback);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="volume">
  {#if volume}
    <label for="volume">
      Volume <span class="muted">{volume.percent}%{decibelText(volume)}</span>
    </label>
    <input
      id="volume"
      type="range"
      min="0"
      max="100"
      step="1"
      value={volume.percent}
      disabled={volume.locked !== null}
      aria-describedby="volume-note"
      oninput={onInput}
    />
    <small id="volume-note" class="muted">
      {#if volume.locked}
        {volume.locked}
      {:else if notBitPerfect}
        Below 0 dB, most USB DACs change the samples: not bit-perfect.
      {:else}
        The device's own volume, shared with the desktop's volume keys; Lyra
        never scales the samples. Ctrl+↑/↓.
      {/if}
    </small>
  {:else if loaded}
    <small class="muted">This device has no volume control.</small>
  {/if}
</div>

<style>
  .volume {
    display: grid;
    gap: 4px;
    margin-top: 10px;
  }
  label {
    font-size: 13px;
  }
  input {
    width: 100%;
    accent-color: var(--accent-line);
  }
  .muted {
    color: var(--text-muted);
    font-size: 12px;
  }
</style>

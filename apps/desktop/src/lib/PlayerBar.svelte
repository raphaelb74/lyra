<script lang="ts">
  import type { Snippet } from 'svelte';

  import { clock, playbackLabel, verdictLabel } from './format';
  import Icon from './Icon.svelte';
  import type { PlayerStatus, TrackDetails } from './ipc';
  import Slider from './Slider.svelte';

  interface Props {
    status: PlayerStatus;
    position: number;
    /** Library data of the playing track, when it comes from the library. */
    details: TrackDetails | null;
    /** Volume and output device, on the right of the transport. */
    output: Snippet;
    onToggle: () => void;
    onPrevious: () => void;
    onNext: () => void;
    onStop: () => void;
    onSeek: (seconds: number) => void;
  }

  const {
    status,
    position,
    details,
    output,
    onToggle,
    onPrevious,
    onNext,
    onStop,
    onSeek,
  }: Props = $props();

  /** Position shown while the user drags the slider. */
  let dragging = $state<number | null>(null);

  const track = $derived(status.track);
  const active = $derived(
    status.state === 'playing' ||
      status.state === 'paused' ||
      status.state === 'buffering',
  );
  const title = $derived(details?.title ?? track?.fileName ?? null);
  const duration = $derived(track?.durationSeconds ?? 0);
  const shown = $derived(dragging ?? position);
  const verdict = $derived(track?.bitPerfect ?? null);
  const verdictText = $derived(verdictLabel(verdict));
  const signal = $derived(track === null ? null : playbackLabel(track));
  const hasNext = $derived(
    track !== null && track.queueIndex + 1 < status.queueLength,
  );
  /** Spoken on change only; position ticks are not announced. */
  const announcement = $derived.by(() => {
    if (status.error !== null) return status.error.message;

    if (title === null) return '';

    if (status.state === 'playing') return `Playing ${title}`;

    if (status.state === 'paused') return `Paused, ${title}`;

    return '';
  });
</script>

<footer aria-label="Player">
  <div class="now">
    {#if details?.art}
      <img class="art" src={details.art} alt="" width="48" height="48" />
    {:else}
      <div class="art" aria-hidden="true"><Icon name="disc-3" /></div>
    {/if}
    <div class="track">
      {#if title !== null}
        <strong class="truncate">{title}</strong>
        {#if details}
          <small class="truncate"
            >{details.artist}{details.album ? ` — ${details.album}` : ''}</small
          >
        {/if}
      {:else}
        <strong>Nothing playing</strong>
        <small class="truncate">Choose an album to start</small>
      {/if}
    </div>
  </div>

  <div class="transport">
    <div class="controls">
      <button
        type="button"
        class="icon"
        aria-label="Previous"
        title="Previous"
        disabled={!active}
        onclick={onPrevious}><Icon name="skip-back" /></button
      >
      <button
        type="button"
        class="icon play"
        aria-label={status.state === 'playing' ? 'Pause' : 'Play'}
        title={status.state === 'playing' ? 'Pause' : 'Play'}
        disabled={!active}
        onclick={onToggle}
        ><Icon name={status.state === 'playing' ? 'pause' : 'play'} /></button
      >
      <button
        type="button"
        class="icon"
        aria-label="Next"
        title="Next"
        disabled={!active || !hasNext}
        onclick={onNext}><Icon name="skip-forward" /></button
      >
      <button
        type="button"
        class="icon stop"
        aria-label="Stop"
        title="Stop"
        disabled={!active}
        onclick={onStop}><Icon name="square" size={14} /></button
      >
    </div>
    <div class="seek num">
      <span>{clock(shown)}</span>
      <Slider
        label="Seek"
        value={shown}
        max={duration}
        step={0.1}
        valueText={`${clock(shown)} of ${clock(duration)}`}
        disabled={!active || duration === 0}
        onInput={(seconds: number) => {
          dragging = seconds;
        }}
        onChange={(seconds: number) => {
          dragging = null;
          onSeek(seconds);
        }}
      />
      <span>{clock(duration)}</span>
    </div>
  </div>

  <div class="output">
    <div class="output-controls">{@render output()}</div>
    {#if active}
      <p class="signal" title={verdictText}>
        <span
          class="lamp"
          data-status={verdict?.status ?? 'none'}
          aria-hidden="true"
        ></span>
        <span class="num">{signal ?? 'Opening…'}</span>
        <span class="visually-hidden">, {verdictText}</span>
      </p>
    {/if}
  </div>

  <p class="visually-hidden" aria-live="polite">{announcement}</p>
</footer>

<style>
  footer {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(280px, 520px) minmax(0, 1fr);
    align-items: center;
    gap: var(--space-5);
    padding: var(--space-3) var(--space-5);
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .now {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .art {
    width: 48px;
    height: 48px;
  }
  .track {
    display: grid;
    gap: var(--space-0);
    min-width: 0;
  }
  strong {
    font-weight: var(--weight-medium);
  }
  small {
    color: var(--text-secondary);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
  }
  .transport {
    display: grid;
    justify-items: center;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .play {
    width: var(--hit-primary);
    height: var(--hit-primary);
    border-radius: var(--radius-round);
    background: var(--text);
    color: var(--bg);
  }
  .play:hover:not(:disabled) {
    background: var(--text-secondary);
    color: var(--bg);
  }
  .play:disabled {
    opacity: 1;
    background: var(--surface-hover);
    color: var(--text-muted);
  }
  .stop {
    color: var(--text-muted);
  }
  .seek {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    font-size: var(--type-caption);
    color: var(--text-muted);
  }
  .seek span {
    min-width: 3.5em;
  }
  .seek span:first-child {
    text-align: right;
  }
  .output {
    display: grid;
    justify-items: end;
    gap: var(--space-0);
    min-width: 0;
  }
  .output-controls {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    width: 100%;
    min-width: 0;
  }
  .signal {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-right: var(--space-2);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    color: var(--text-secondary);
  }
  /* The one lit element: bit-perfect verified by the engine. */
  .lamp {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-round);
    border: 1px solid var(--text-muted);
  }
  .lamp[data-status='yes'] {
    border-color: var(--accent);
    background: var(--accent);
    box-shadow: 0 0 4px var(--accent);
  }
  .lamp[data-status='no'] {
    border-color: var(--warning);
    background: var(--warning);
  }
  @media (max-width: 1100px) {
    footer {
      grid-template-columns:
        minmax(0, 0.8fr)
        minmax(200px, 320px)
        minmax(0, 1.2fr);
      gap: var(--space-4);
    }
    small {
      display: none;
    }
  }
</style>

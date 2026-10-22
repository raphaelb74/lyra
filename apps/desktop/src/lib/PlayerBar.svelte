<script lang="ts">
  import type { Snippet } from 'svelte';

  import { clock, playbackLabel, verdictLabel } from './format';
  import Icon from './Icon.svelte';
  import type { PlayerStatus, Repeat, TrackDetails } from './ipc';
  import Slider from './Slider.svelte';
  import { readFlag, writeFlag } from './storage';

  interface Props {
    status: PlayerStatus;
    position: number;
    /** Library data of the playing track, when it comes from the library. */
    details: TrackDetails | null;
    /** Volume and output device, on the right of the transport. */
    output: Snippet;
    /** Shows Now Playing. */
    onOpen: () => void;
    onToggle: () => void;
    onPrevious: () => void;
    onNext: () => void;
    onStop: () => void;
    onShuffle: (on: boolean) => void;
    onRepeat: (mode: Repeat) => void;
    onSeek: (seconds: number) => void;
  }

  const {
    status,
    position,
    details,
    output,
    onOpen,
    onToggle,
    onPrevious,
    onNext,
    onStop,
    onShuffle,
    onRepeat,
    onSeek,
  }: Props = $props();

  /** Each press moves to the next mode, as in other players. */
  const NEXT_REPEAT: Record<Repeat, Repeat> = {
    off: 'all',
    all: 'one',
    one: 'off',
  };
  const REPEAT_LABEL: Record<Repeat, string> = {
    off: 'Repeat: off',
    all: 'Repeat: all',
    one: 'Repeat: this track',
  };
  const TIME_LEFT_KEY = 'lyra.player.timeLeft';

  /** The end of the seek bar shows the time left instead of the length. */
  let timeLeft = $state(readFlag(TIME_LEFT_KEY));

  function toggleTimeLeft() {
    timeLeft = !timeLeft;
    writeFlag(TIME_LEFT_KEY, timeLeft);
  }

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
  const end = $derived(
    timeLeft && duration > 0
      ? `\u2212${clock(Math.max(0, duration - shown))}`
      : clock(duration),
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
  <button
    type="button"
    class="now"
    aria-label={title === null ? 'Now playing' : `Now playing: ${title}`}
    title="Now playing"
    onclick={onOpen}
  >
    {#if details?.thumb}
      <img class="art" src={details.thumb} alt="" width="48" height="48" />
    {:else}
      <span class="art" aria-hidden="true"><Icon name="disc-3" /></span>
    {/if}
    <span class="track">
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
    </span>
  </button>

  <div class="controls">
    <div class="side start">
      <button
        type="button"
        class="icon mode"
        aria-label="Shuffle"
        aria-pressed={status.shuffle}
        title={status.shuffle ? 'Shuffle: on' : 'Shuffle: off'}
        onclick={() => {
          onShuffle(!status.shuffle);
        }}><Icon name="shuffle" size={16} /></button
      >
      <button
        type="button"
        class="icon"
        aria-label="Previous"
        title="Previous"
        disabled={!active}
        onclick={onPrevious}><Icon name="skip-back" /></button
      >
    </div>
    <button
      type="button"
      class="icon play"
      aria-label={status.state === 'playing' ? 'Pause' : 'Play'}
      title={status.state === 'playing' ? 'Pause' : 'Play'}
      disabled={!active}
      onclick={onToggle}
      ><Icon name={status.state === 'playing' ? 'pause' : 'play'} /></button
    >
    <div class="side">
      <button
        type="button"
        class="icon"
        aria-label="Next"
        title="Next"
        disabled={!active || !status.hasNext}
        onclick={onNext}><Icon name="skip-forward" /></button
      >
      <button
        type="button"
        class="icon mode"
        aria-label={REPEAT_LABEL[status.repeat]}
        aria-pressed={status.repeat !== 'off'}
        title={REPEAT_LABEL[status.repeat]}
        onclick={() => {
          onRepeat(NEXT_REPEAT[status.repeat]);
        }}
        ><Icon
          name={status.repeat === 'one' ? 'repeat-1' : 'repeat'}
          size={16}
        /></button
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
  </div>
  <div class="seek num">
    <span class="elapsed">{clock(shown)}</span>
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
    <button
      type="button"
      class="time"
      aria-label={timeLeft
        ? 'Time left; show length'
        : 'Length; show time left'}
      title={timeLeft ? 'Show length' : 'Show time left'}
      onclick={toggleTimeLeft}>{end}</button
    >
  </div>

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

  <p class="visually-hidden" aria-live="polite">{announcement}</p>
</footer>

<style>
  footer {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(280px, 520px) minmax(0, 1fr);
    /* Buttons and volume on one row, seek bar and signal on the next. */
    grid-template-rows: auto auto;
    align-items: center;
    column-gap: var(--space-5);
    row-gap: var(--space-1);
    padding: var(--space-3) var(--space-5);
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .now {
    grid-row: 1 / span 2;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: var(--space-1);
    margin-left: calc(-1 * var(--space-1));
    border: 0;
    text-align: left;
    font-weight: var(--weight-regular);
  }
  .art {
    width: 48px;
    height: 48px;
  }
  .track {
    min-width: 0;
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
  /* Play sits on the centre line, over the middle of the seek bar. */
  .controls {
    grid-column: 2;
    grid-row: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-2);
  }
  .side {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .side.start {
    justify-content: flex-end;
  }
  /* Off, quieter than the transport; on, the shared toggle style. */
  .mode:not([aria-pressed='true']) {
    color: var(--text-muted);
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
    grid-column: 2;
    grid-row: 2;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    font-size: var(--type-caption);
    color: var(--text-muted);
  }
  .elapsed,
  .time {
    min-width: 4em;
  }
  .elapsed {
    text-align: right;
  }
  .time {
    padding: 0 var(--space-1);
    margin-left: calc(-1 * var(--space-1));
    border: 0;
    border-radius: var(--radius-s);
    font: inherit;
    text-align: left;
  }
  .time:hover {
    background: transparent;
    color: var(--text-secondary);
  }
  .output-controls {
    grid-column: 3;
    grid-row: 1;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    width: 100%;
    min-width: 0;
  }
  .signal {
    grid-column: 3;
    grid-row: 2;
    justify-self: end;
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

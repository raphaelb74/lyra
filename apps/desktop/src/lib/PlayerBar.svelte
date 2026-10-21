<script lang="ts">
  import { clock, outputLabel, sourceLabel, verdictLabel } from './format';
  import type { DeviceInfo, PlayerStatus, TrackDetails } from './ipc';

  interface Props {
    status: PlayerStatus;
    position: number;
    device: DeviceInfo | undefined;
    /** Library data of the playing track, when it comes from the library. */
    details: TrackDetails | null;
    onToggle: () => void;
    onPrevious: () => void;
    onNext: () => void;
    onStop: () => void;
    onSeek: (seconds: number) => void;
  }

  const {
    status,
    position,
    device,
    details,
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
  const duration = $derived(track?.durationSeconds ?? 0);
  const shown = $derived(dragging ?? position);
  const verdict = $derived(track?.bitPerfect ?? null);
  const verdictText = $derived(verdictLabel(verdict));
  const hasNext = $derived(
    track !== null && track.queueIndex + 1 < status.queueLength,
  );
</script>

<footer aria-label="Player">
  {#if details?.art}
    <img class="art" src={details.art} alt="" width="54" height="54" />
  {:else}
    <div class="art" aria-hidden="true">♪</div>
  {/if}
  <div class="track">
    {#if track}
      <strong>{details?.title ?? track.fileName}</strong>
      <small
        >{details ? `${details.artist} · ` : ''}{sourceLabel(track)} · {status.state}</small
      >
    {:else}
      <strong>Nothing playing</strong>
      <small>Choose an album, or drop files onto the window</small>
    {/if}
  </div>
  <div class="transport">
    <div class="controls">
      <button
        type="button"
        aria-label="Previous"
        disabled={!active}
        onclick={onPrevious}>⏮</button
      >
      <button
        type="button"
        aria-label={status.state === 'paused' ? 'Resume' : 'Play'}
        aria-pressed={status.state === 'playing'}
        disabled={!active}
        onclick={onToggle}>{status.state === 'playing' ? '❚❚' : '▶'}</button
      >
      <button
        type="button"
        aria-label="Stop"
        disabled={!active}
        onclick={onStop}>■</button
      >
      <button
        type="button"
        aria-label="Next"
        disabled={!active || !hasNext}
        onclick={onNext}>⏭</button
      >
    </div>
    <div class="seek">
      <span>{clock(shown)}</span>
      <input
        type="range"
        aria-label="Seek"
        min="0"
        max={duration}
        step="0.1"
        value={shown}
        disabled={!active || duration === 0}
        oninput={(event) => {
          dragging = event.currentTarget.valueAsNumber;
        }}
        onchange={(event) => {
          dragging = null;
          onSeek(event.currentTarget.valueAsNumber);
        }}
      />
      <span>{clock(duration)}</span>
    </div>
  </div>
  <div class="output">
    <strong>{device?.cardName ?? 'No DAC selected'}</strong>
    <small
      >{track?.output ? outputLabel(track.output) : 'Output inactive'}</small
    >
    <small
      class="verdict"
      data-status={verdict?.status ?? 'none'}
      title={verdictText}>{verdictText}</small
    >
  </div>
</footer>

<style>
  footer {
    min-height: 100px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 18px 26px;
    background: var(--surface-raised);
    border-top: 1px solid var(--border-player);
  }
  .art {
    background: var(--art-placeholder);
    color: var(--accent);
    border-radius: 8px;
    width: 54px;
    height: 54px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
    font-size: 24px;
    object-fit: cover;
  }
  .track,
  .output {
    display: flex;
    flex-direction: column;
    gap: 7px;
    font-size: 13px;
    min-width: 0;
    width: 260px;
  }
  .track strong,
  .output strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small {
    color: var(--text-muted);
    font-size: 11px;
  }
  .transport {
    flex: 1;
    display: grid;
    gap: 8px;
    justify-items: center;
  }
  .controls {
    display: flex;
    gap: 10px;
  }
  .controls button {
    border: 0;
    background: transparent;
    min-width: 44px;
  }
  .seek {
    display: flex;
    align-items: center;
    gap: 10px;
    width: min(100%, 520px);
    font-size: 11px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .seek input {
    flex: 1;
    accent-color: var(--accent);
  }
  .output {
    text-align: right;
  }
  .verdict[data-status='yes'] {
    color: var(--accent);
  }
  .verdict[data-status='no'] {
    color: var(--warning);
  }
  @media (max-width: 650px) {
    footer {
      gap: 10px;
      padding: 15px;
    }
    .art,
    .output {
      display: none;
    }
  }
</style>

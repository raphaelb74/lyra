<script lang="ts">
  import { clock, outputLabel, sourceLabel } from './format';
  import type { DeviceInfo, PlayerStatus } from './ipc';

  interface Props {
    status: PlayerStatus;
    position: number;
    device: DeviceInfo | undefined;
    onToggle: () => void;
    onStop: () => void;
    onSeek: (seconds: number) => void;
  }

  const { status, position, device, onToggle, onStop, onSeek }: Props =
    $props();

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
  const verdictText = $derived.by(() => {
    if (verdict === null) return 'Bit-perfect: not playing';

    if (verdict.status === 'yes') return 'Bit-perfect';

    const reason = verdict.reason ?? 'unknown';

    return verdict.status === 'no'
      ? `Not bit-perfect: ${reason}`
      : `Bit-perfect unverified: ${reason}`;
  });
</script>

<footer aria-label="Player">
  <div class="art" aria-hidden="true">♪</div>
  <div class="track">
    {#if track}
      <strong>{track.fileName}</strong>
      <small>{sourceLabel(track)} · {status.state}</small>
    {:else}
      <strong>Nothing playing</strong>
      <small>Drop a FLAC or WAV file onto the window</small>
    {/if}
  </div>
  <div class="transport">
    <div class="controls">
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
    background: #1a232b;
    border-top: 1px solid #303c46;
  }
  .art {
    background: #31463f;
    color: #b4d9cb;
    border-radius: 8px;
    width: 54px;
    height: 54px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
    font-size: 24px;
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
    color: #a2adb7;
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
    color: #a2adb7;
    font-variant-numeric: tabular-nums;
  }
  .seek input {
    flex: 1;
    accent-color: #b4d9cb;
  }
  .output {
    text-align: right;
  }
  .verdict[data-status='yes'] {
    color: #b4d9cb;
  }
  .verdict[data-status='no'] {
    color: #f0b48a;
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

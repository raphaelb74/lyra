<script lang="ts">
  import { playbackLabel, verdictLabel } from './format';
  import Icon from './Icon.svelte';
  import type { DeviceInfo, PlayerStatus, TrackDetails } from './ipc';
  import Lamp from './Lamp.svelte';
  import SignalPath from './SignalPath.svelte';
  import { readFlag, writeFlag } from './storage';

  interface Props {
    status: PlayerStatus;
    details: TrackDetails | null;
    /** The device the track plays on. */
    device: DeviceInfo | undefined;
    onBack: () => void;
  }

  const { status, details, device, onBack }: Props = $props();

  const SIGNAL_OPEN_KEY = 'lyra.nowPlaying.signalPath';

  const track = $derived(status.track);
  const signal = $derived(track === null ? null : playbackLabel(track));
  /** Collapsed until asked for; remembered between launches. */
  let signalOpen = $state(readFlag(SIGNAL_OPEN_KEY));

  function toggleSignal() {
    signalOpen = !signalOpen;
    writeFlag(SIGNAL_OPEN_KEY, signalOpen);
  }
</script>

<button type="button" class="back" onclick={onBack}
  ><Icon name="chevron-left" size={16} />Back</button
>
<section class="now" aria-labelledby="now-title">
  {#if track}
    {#if details?.art}
      <img class="art" src={details.art} alt="" width="560" height="560" />
    {:else}
      <div class="art" aria-hidden="true"><Icon name="disc-3" size={64} /></div>
    {/if}
    <div class="info">
      <h3 id="now-title">{details?.title ?? track.fileName}</h3>
      {#if details}
        <p class="artist">{details.artist}</p>
        {#if details.album}
          <p class="album">
            {details.album}{details.year === null
              ? ''
              : ` · ${String(details.year)}`}
          </p>
        {/if}
      {/if}

      <p class="format" title={verdictLabel(track.bitPerfect)}>
        <Lamp {track} />
        <span class="num" data-status={track.bitPerfect?.status ?? 'none'}
          >{signal ?? 'Opening…'}</span
        >
        <span class="visually-hidden">, {verdictLabel(track.bitPerfect)}</span>
      </p>

      <button
        type="button"
        class="disclosure"
        aria-expanded={signalOpen}
        aria-controls="now-signal"
        onclick={toggleSignal}
        ><Icon name="chevron-down" size={16} />
        Signal path</button
      >
      {#if signalOpen}
        <div id="now-signal" class="signal">
          <SignalPath {track} {device} />
        </div>
      {/if}

      <p class="queue muted">
        Track {track.queueIndex + 1} of {status.queueLength}
      </p>
    </div>
  {:else}
    <div class="info">
      <h3 id="now-title">Nothing playing</h3>
      <p class="muted">Choose an album to start.</p>
    </div>
  {/if}
</section>

<style>
  .back {
    margin-bottom: var(--space-4);
  }
  .now {
    display: flex;
    gap: var(--space-6);
    align-items: flex-start;
    flex-wrap: wrap;
  }
  /* As large as the window's height allows (title bar, player bar and
     margins taken off), up to 560 px. */
  .art {
    width: min(560px, 100%, calc(100dvh - 260px));
    min-width: 200px;
    height: auto;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: var(--radius-m);
    display: grid;
    place-items: center;
    background: var(--art-empty);
    color: var(--text-muted);
  }
  .info {
    flex: 1;
    min-width: 280px;
    max-width: 640px;
  }
  h3 {
    font-size: var(--type-display);
    line-height: var(--leading-display);
    font-weight: var(--weight-strong);
    letter-spacing: -0.01em;
    margin: 0 0 var(--space-3);
  }
  p {
    margin: var(--space-1) 0;
  }
  .artist {
    font-size: var(--type-title);
    line-height: var(--leading-title);
    color: var(--text);
  }
  .album {
    color: var(--text-secondary);
  }
  .format {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-5) 0 var(--space-3);
    font-size: var(--type-small);
    color: var(--text-secondary);
  }
  .num[data-status='no'] {
    color: var(--warning);
  }
  .disclosure {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2) var(--space-1) 0;
    border: 0;
    color: var(--text-secondary);
    font-weight: var(--weight-medium);
  }
  .disclosure[aria-expanded='false'] :global(svg) {
    transform: rotate(-90deg);
  }
  .disclosure:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }
  .signal {
    margin: var(--space-3) 0 var(--space-2);
    padding: var(--space-4);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
  }
  .queue {
    margin-top: var(--space-4);
    font-size: var(--type-small);
  }
</style>

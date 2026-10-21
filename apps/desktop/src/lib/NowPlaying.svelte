<script lang="ts">
  import { playbackLabel, sourceLabel, verdictLabel } from './format';
  import Icon from './Icon.svelte';
  import type { DeviceInfo, PlayerStatus, TrackDetails } from './ipc';

  interface Props {
    status: PlayerStatus;
    details: TrackDetails | null;
    device: DeviceInfo | undefined;
  }

  const { status, details, device }: Props = $props();

  const track = $derived(status.track);
  const signal = $derived(track === null ? null : playbackLabel(track));
</script>

<section class="now" aria-labelledby="now-title">
  {#if track}
    {#if details?.art}
      <img class="art" src={details.art} alt="" width="320" height="320" />
    {:else}
      <div class="art" aria-hidden="true"><Icon name="disc-3" size={64} /></div>
    {/if}
    <div class="info">
      <h3 id="now-title">{details?.title ?? track.fileName}</h3>
      {#if details}
        <p>{details.artist}</p>
        {#if details.album}
          <p class="muted">
            {details.album}{details.year === null
              ? ''
              : ` · ${String(details.year)}`}
          </p>
        {/if}
      {/if}
      <ul class="badges" aria-label="Signal path">
        <li>{sourceLabel(track)}</li>
        {#if signal}
          <li class="transport">{signal}</li>
        {/if}
      </ul>
      <dl>
        <dt>Device</dt>
        <dd>
          {device?.label ?? track.device}
        </dd>
        <dt>Status</dt>
        <dd class="verdict" data-status={track.bitPerfect?.status ?? 'none'}>
          {verdictLabel(track.bitPerfect)}
        </dd>
        <dt>Queue</dt>
        <dd>
          Track {track.queueIndex + 1} of {status.queueLength}
        </dd>
      </dl>
    </div>
  {:else}
    <h3 id="now-title">Nothing playing</h3>
    <p class="muted">Choose an album in the library to start.</p>
  {/if}
</section>

<style>
  .now {
    display: flex;
    gap: var(--space-6);
    align-items: flex-start;
    flex-wrap: wrap;
  }
  .art {
    width: min(320px, 100%);
    height: auto;
  }
  .info {
    flex: 1;
    min-width: 260px;
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
    font-size: var(--type-title);
    line-height: var(--leading-title);
    color: var(--text-secondary);
  }
  p.muted {
    font-size: var(--type-body);
  }
  .badges {
    list-style: none;
    padding: 0;
    margin: var(--space-5) 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .badges li {
    border: 1px solid var(--line-strong);
    color: var(--text-secondary);
    border-radius: var(--radius-s);
    padding: var(--space-1) var(--space-2);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    font-variant-numeric: tabular-nums;
  }
  .badges .transport {
    color: var(--text);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-5);
    font-size: var(--type-small);
    margin: 0;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
  }
  .verdict[data-status='yes'] {
    color: var(--accent);
  }
  .verdict[data-status='no'] {
    color: var(--warning);
  }
</style>

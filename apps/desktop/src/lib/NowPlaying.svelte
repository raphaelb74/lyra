<script lang="ts">
  import { outputLabel, sourceLabel, verdictLabel } from './format';
  import type { DeviceInfo, PlayerStatus, TrackDetails } from './ipc';

  interface Props {
    status: PlayerStatus;
    details: TrackDetails | null;
    device: DeviceInfo | undefined;
  }

  const { status, details, device }: Props = $props();

  const track = $derived(status.track);
  const transport = $derived.by(() => {
    switch (track?.output?.transport) {
      case 'native':
        return 'Native DSD';
      case 'dop':
        return 'DoP';
      case 'pcm':
        return 'PCM';
      default:
        return null;
    }
  });
</script>

<section class="now" aria-labelledby="now-title">
  {#if track}
    {#if details?.art}
      <img src={details.art} alt="" width="320" height="320" />
    {:else}
      <div class="placeholder" aria-hidden="true">♪</div>
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
        {#if transport}
          <li class="transport">{transport}</li>
        {/if}
        {#if track.output}
          <li>{outputLabel(track.output)}</li>
        {/if}
      </ul>
      <dl>
        <dt>Device</dt>
        <dd>
          {device?.cardName ?? track.device}
          <small>{track.device}</small>
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
    gap: 40px;
    align-items: flex-start;
    flex-wrap: wrap;
  }
  img,
  .placeholder {
    width: min(320px, 100%);
    height: auto;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 10px;
    background: var(--surface);
  }
  .placeholder {
    display: grid;
    place-items: center;
    font-size: 72px;
    color: var(--accent-line);
  }
  .info {
    flex: 1;
    min-width: 260px;
  }
  h3 {
    font-size: clamp(24px, 3vw, 36px);
    font-weight: 500;
    margin: 0 0 12px;
    letter-spacing: -0.5px;
  }
  p {
    margin: 4px 0;
    font-size: 16px;
  }
  .muted {
    color: var(--text-muted);
    font-size: 14px;
  }
  .badges {
    list-style: none;
    padding: 0;
    margin: 24px 0;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .badges li {
    border: 1px solid var(--border-strong);
    color: var(--accent);
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 12px;
  }
  .badges .transport {
    background: var(--surface-selected);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 10px 20px;
    font-size: 13px;
    margin: 0;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
  }
  dd small {
    display: block;
    color: var(--text-muted);
    font-size: 11px;
    margin-top: 2px;
  }
  .verdict[data-status='yes'] {
    color: var(--accent);
  }
  .verdict[data-status='no'] {
    color: var(--warning);
  }
</style>

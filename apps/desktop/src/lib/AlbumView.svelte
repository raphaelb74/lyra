<script lang="ts">
  import { clock, formatBadge } from './format';
  import { attempt, type Album, type Backend, type LibraryTrack } from './ipc';

  interface Props {
    album: Album;
    api: Backend;
    /** Selected output device; empty when none. */
    device: string;
    /** Library id of the playing track, to mark it. */
    playingId: number | null;
    onBack: () => void;
    onError: (message: string) => void;
  }

  const { album, api, device, playingId, onBack, onError }: Props = $props();

  let tracks = $state<LibraryTrack[]>([]);
  let loaded = $state(false);

  const discs = $derived(new Set(tracks.map((t) => t.disc ?? 1)).size > 1);

  $effect(() => {
    const id = album.id;

    loaded = false;
    void attempt(() => api.albumTracks(id), onError).then((list) => {
      tracks = list ?? [];
      loaded = true;
    });
  });

  /** Plays the album from `index`, so it continues through the rest. */
  function play(index: number) {
    const ids = tracks.map((t) => t.id);

    void attempt(() => api.playTracks(ids, index, device), onError);
  }

  function number(track: LibraryTrack): string {
    const position = track.track === null ? '·' : String(track.track);

    return discs ? `${String(track.disc ?? 1)}.${position}` : position;
  }
</script>

<section class="album" aria-labelledby="album-title">
  <button type="button" class="back" onclick={onBack}>← All albums</button>
  <header>
    {#if album.art}
      <img src={album.art} alt="" width="180" height="180" />
    {:else}
      <div class="placeholder" aria-hidden="true">♪</div>
    {/if}
    <div>
      <h3 id="album-title">{album.title}</h3>
      <p>
        {album.artist}{album.year === null ? '' : ` · ${String(album.year)}`}
      </p>
      <p class="muted">
        {album.trackCount}
        {album.trackCount === 1 ? 'track' : 'tracks'} · {formatBadge(album)}
      </p>
    </div>
  </header>
  {#if loaded && tracks.length === 0}
    <p class="muted">This album has no tracks any more.</p>
  {/if}
  <ol>
    {#each tracks as track, index (track.id)}
      <li>
        <button
          type="button"
          aria-current={track.id === playingId ? 'true' : undefined}
          disabled={device === ''}
          title={device === '' ? 'Choose an output device first' : undefined}
          onclick={() => {
            play(index);
          }}
        >
          <span class="number">{number(track)}</span>
          <span class="title">
            {track.title}
            {#if track.artist !== album.artist}
              <small>{track.artist}</small>
            {/if}
          </span>
          <span class="format">{formatBadge(track)}</span>
          <span class="time"
            >{track.durationSeconds === null
              ? ''
              : clock(track.durationSeconds)}</span
          >
        </button>
      </li>
    {/each}
  </ol>
</section>

<style>
  .back {
    border: 0;
    background: transparent;
    padding: 6px 0;
    color: #a2adb7;
  }
  header {
    display: flex;
    gap: 24px;
    align-items: flex-end;
    margin: 16px 0 24px;
  }
  img,
  .placeholder {
    width: 180px;
    height: 180px;
    border-radius: 8px;
    object-fit: cover;
    flex-shrink: 0;
    background: #182129;
  }
  .placeholder {
    display: grid;
    place-items: center;
    font-size: 48px;
    color: #577369;
  }
  h3 {
    font-size: 28px;
    font-weight: 500;
    margin: 0 0 8px;
  }
  header p {
    margin: 4px 0;
  }
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  ol {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 2px;
  }
  ol button {
    width: 100%;
    display: grid;
    grid-template-columns: 3em minmax(0, 1fr) auto 4em;
    gap: 16px;
    align-items: center;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 10px 12px;
  }
  ol button[aria-current='true'] {
    background: #1f2d2a;
    color: #d9eee4;
  }
  ol button:hover:not(:disabled),
  ol button:focus-visible {
    background: #182129;
  }
  .number,
  .time,
  .format {
    color: #a2adb7;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .time {
    text-align: right;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title small {
    color: #a2adb7;
    margin-left: 8px;
  }
  @media (max-width: 650px) {
    header {
      flex-direction: column;
      align-items: flex-start;
    }
    ol button {
      grid-template-columns: 2.5em minmax(0, 1fr) 3.5em;
    }
    .format {
      display: none;
    }
  }
</style>

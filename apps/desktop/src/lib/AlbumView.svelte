<script lang="ts">
  import AddToPlaylist from './AddToPlaylist.svelte';
  import { clock, formatBadge } from './format';
  import Icon from './Icon.svelte';
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
  <button type="button" class="back" onclick={onBack}
    ><Icon name="chevron-left" size={16} />All albums</button
  >
  <header>
    {#if album.art}
      <img class="art" src={album.art} alt="" width="200" height="200" />
    {:else}
      <div class="art" aria-hidden="true"><Icon name="disc-3" size={48} /></div>
    {/if}
    <div>
      <h3 id="album-title">{album.title}</h3>
      <p>
        {album.artist}{album.year === null ? '' : ` · ${String(album.year)}`}
      </p>
      <p class="muted num">
        {album.trackCount}
        {album.trackCount === 1 ? 'track' : 'tracks'} · {formatBadge(album)}
      </p>
      <div class="actions">
        <button
          type="button"
          class="primary"
          disabled={tracks.length === 0 || device === ''}
          onclick={() => {
            play(0);
          }}>Play album</button
        >
        <AddToPlaylist
          {api}
          trackIds={tracks.map((t) => t.id)}
          label={`Add ${album.title} to a playlist`}
          text="Add to playlist"
          {onError}
        />
      </div>
    </div>
  </header>
  {#if loaded && tracks.length === 0}
    <p class="muted">This album has no tracks any more.</p>
  {/if}
  <ol class="rows">
    {#each tracks as track, index (track.id)}
      <li>
        <button
          type="button"
          class="row"
          aria-current={track.id === playingId ? 'true' : undefined}
          disabled={device === ''}
          title={device === '' ? 'Choose an output device first' : undefined}
          onclick={() => {
            play(index);
          }}
        >
          <span class="meta end">{number(track)}</span>
          <span class="title">
            {track.title}
            {#if track.artist !== album.artist}
              <small>{track.artist}</small>
            {/if}
          </span>
          <span class="meta format">{formatBadge(track)}</span>
          <span class="meta end"
            >{track.durationSeconds === null
              ? ''
              : clock(track.durationSeconds)}</span
          >
        </button>
        <AddToPlaylist
          {api}
          trackIds={[track.id]}
          label={`Add ${track.title} to a playlist`}
          {onError}
        />
      </li>
    {/each}
  </ol>
</section>

<style>
  header {
    display: flex;
    gap: var(--space-5);
    align-items: flex-end;
    margin: var(--space-4) 0 var(--space-5);
  }
  .art {
    width: 200px;
    height: 200px;
  }
  h3 {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
    margin: 0 0 var(--space-2);
  }
  header p {
    margin: var(--space-1) 0;
    color: var(--text-secondary);
  }
  .actions {
    margin-top: var(--space-3);
  }
  .rows {
    --columns: 3em minmax(0, 1fr) auto 4em;
  }
  @media (max-width: 650px) {
    header {
      flex-direction: column;
      align-items: flex-start;
    }
    .rows {
      --columns: 2.5em minmax(0, 1fr) 3.5em;
    }
    .format {
      display: none;
    }
  }
</style>

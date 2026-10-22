<script lang="ts">
  import AddToPlaylist from './AddToPlaylist.svelte';
  import {
    albumFormats,
    albumLength,
    clock,
    formatBadge,
    isHiRes,
  } from './format';
  import FormatLogo from './FormatLogo.svelte';
  import Icon from './Icon.svelte';
  import { attempt, type Album, type Backend, type LibraryTrack } from './ipc';
  import { addToPlaylist, openMenu, queueActions } from './menu.svelte';
  import { rowEvents } from './rows';

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

  /** The row a click selected; a double click plays. */
  let selected = $state<number | null>(null);

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

  const formats = $derived(albumFormats(tracks));
  const hiRes = $derived(tracks.some(isHiRes));
  /** Format logos for the families among the tracks (WAV has none). */
  const logos = $derived(
    (['dsd', 'flac'] as const).filter((family) =>
      tracks.some((track) =>
        family === 'dsd' ? track.dsdMultiple !== null : track.codec === 'flac',
      ),
    ),
  );
  /** `1973 · Rock · 10 tracks, 38 min`. */
  const facts = $derived(
    [
      album.year === null ? null : String(album.year),
      album.genre,
      `${String(album.trackCount)} ${album.trackCount === 1 ? 'track' : 'tracks'}${
        album.durationSeconds === null
          ? ''
          : `, ${albumLength(album.durationSeconds)}`
      }`,
    ]
      .filter((fact) => fact !== null)
      .join(' · '),
  );

  /** Plays the album from `index`, so it continues through the rest. */
  function play(index: number) {
    const ids = tracks.map((t) => t.id);

    void attempt(() => api.playTracks(ids, index, device), onError);
  }

  /** Turns shuffle on and starts from a random track. */
  function shuffle() {
    const ids = tracks.map((t) => t.id);
    const start = Math.floor(Math.random() * ids.length);

    void attempt(async () => {
      await api.setShuffle(true);

      return api.playTracks(ids, start, device);
    }, onError);
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
    <div class="details">
      <h3 id="album-title">{album.title}</h3>
      <p class="artist">{album.artist}</p>
      <p class="muted num">{facts}</p>
      {#if formats.length > 0}
        <div class="formats">
          {#each logos as logo (logo)}<FormatLogo format={logo} />{/each}
          <ul class="badges" aria-label="Formats">
            {#if hiRes}<li class="hi-res">Hi-Res</li>{/if}
            {#each formats as format (format)}
              <li class="num">{format}</li>
            {/each}
          </ul>
        </div>
      {/if}
      <div class="actions">
        <button
          type="button"
          class="primary with-icon"
          disabled={tracks.length === 0 || device === ''}
          onclick={() => {
            play(0);
          }}><Icon name="play" size={16} />Play</button
        >
        <button
          type="button"
          class="with-icon"
          disabled={tracks.length === 0 || device === ''}
          onclick={shuffle}><Icon name="shuffle" size={16} />Shuffle</button
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
          data-selected={selected === track.id}
          {...rowEvents(
            () => (selected = track.id),
            () => {
              play(index);
            },
          )}
          oncontextmenu={(event) => {
            openMenu(event, [
              {
                label: 'Play',
                icon: 'play',
                disabled: device === '',
                run: () => {
                  play(index);
                },
              },
              ...queueActions(
                api,
                () => Promise.resolve([track.id]),
                device,
                onError,
              ),
              'separator',
              addToPlaylist(api, () => Promise.resolve([track.id]), onError),
            ]);
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
  /* The details go under the cover when the view is narrow (queue open). */
  header {
    display: flex;
    flex-wrap: wrap;
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
  header .artist {
    color: var(--text);
    font-size: var(--type-title);
    line-height: var(--leading-title);
  }
  /* Format logos, then tags: a quiet outline; Hi-Res in the accent colour. */
  .formats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
    color: var(--text-secondary);
  }
  .badges {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
  }
  .badges li {
    padding: var(--space-0) var(--space-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-s);
    color: var(--text-secondary);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    font-weight: var(--weight-medium);
    letter-spacing: 0.04em;
  }
  .badges .hi-res {
    border-color: var(--accent);
    color: var(--accent);
  }
  .details {
    flex: 1 1 320px;
    min-width: 0;
  }
  .actions {
    flex-wrap: wrap;
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

<script lang="ts">
  import AlbumGrid from './AlbumGrid.svelte';
  import Icon from './Icon.svelte';
  import { attempt, type Album, type Artist, type Backend } from './ipc';
  import { openMenu } from './menu.svelte';

  interface Props {
    api: Backend;
    /** Selected output device; empty when none. */
    device: string;
    /** Changes when the library changed; reloads the list. */
    version: number;
    /** Chosen artist, kept by the parent so it survives opening an album. */
    selectedId: number | null;
    onSelect: (id: number) => void;
    onOpenAlbum: (album: Album) => void;
    onAlbumMenu: (event: MouseEvent, album: Album) => void;
    onError: (message: string) => void;
  }

  const {
    api,
    device,
    version,
    selectedId,
    onSelect,
    onOpenAlbum,
    onAlbumMenu,
    onError,
  }: Props = $props();

  let artists = $state<Artist[]>([]);
  let loaded = $state(false);
  let albums = $state<Album[]>([]);
  let list = $state<HTMLUListElement | undefined>();

  /** The chosen artist, or the first one. */
  const current = $derived(
    artists.find((artist) => artist.id === selectedId) ?? artists[0],
  );

  /** Loads for library `forVersion`; a reply for an older one is dropped. */
  async function loadArtists(forVersion: number) {
    const found = await attempt(api.artists, onError);

    if (forVersion !== version) return;

    artists = found ?? [];
    loaded = true;
  }

  async function loadAlbums(id: number) {
    const found = await attempt(() => api.artistAlbums(id), onError);

    if (current?.id === id) albums = found ?? [];
  }

  /** Plays every album of the artist, oldest first. */
  async function playArtist(id: number) {
    const parts = await attempt(async () => {
      const found = await api.artistAlbums(id);

      return Promise.all(found.map((album) => api.albumTracks(album.id)));
    }, onError);

    const ids = (parts ?? []).flat().map((track) => track.id);

    if (ids.length > 0) {
      void attempt(() => api.playTracks(ids, 0, device), onError);
    }
  }

  /** ↑ and ↓ move through the list, as in a native list view. */
  function onKey(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;

    const index = artists.findIndex((artist) => artist.id === current?.id);
    const next = artists[index + (event.key === 'ArrowDown' ? 1 : -1)];

    if (next === undefined) return;

    event.preventDefault();
    onSelect(next.id);
    list
      ?.querySelector<HTMLButtonElement>(`[data-id="${String(next.id)}"]`)
      ?.focus();
  }

  $effect(() => {
    void loadArtists(version);
  });

  $effect(() => {
    const id = current?.id;

    albums = [];

    if (id !== undefined) void loadAlbums(id);
  });
</script>

{#if loaded && artists.length === 0}
  <p class="muted">No artists yet. Add a music folder in Settings.</p>
{:else}
  <div class="artists">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul class="list" aria-label="Artists" bind:this={list} onkeydown={onKey}>
      {#each artists as artist (artist.id)}
        <li>
          <button
            type="button"
            class="artist"
            data-id={artist.id}
            aria-current={artist.id === current?.id ? 'true' : undefined}
            onclick={() => {
              onSelect(artist.id);
            }}
            oncontextmenu={(event) => {
              openMenu(event, [
                {
                  label: `Play ${artist.name}`,
                  icon: 'play',
                  disabled: device === '',
                  run: () => void playArtist(artist.id),
                },
              ]);
            }}
          >
            {#if artist.art}
              <img
                class="avatar"
                src={artist.art}
                alt=""
                loading="lazy"
                width="36"
                height="36"
              />
            {:else}
              <span class="avatar" aria-hidden="true"
                ><Icon name="mic-vocal" size={16} /></span
              >
            {/if}
            <span class="truncate">{artist.name}</span>
          </button>
        </li>
      {/each}
    </ul>

    <div class="detail">
      {#if current}
        <header>
          <div class="title">
            <h2>{current.name}</h2>
            <p class="muted num">
              {current.albumCount}
              {current.albumCount === 1 ? 'album' : 'albums'} · {current.trackCount}
              {current.trackCount === 1 ? 'track' : 'tracks'}
            </p>
          </div>
          <button
            type="button"
            class="icon play"
            aria-label={`Play ${current.name}`}
            title="Play"
            disabled={device === '' || albums.length === 0}
            onclick={() => void playArtist(current.id)}
            ><Icon name="play" /></button
          >
        </header>
        <AlbumGrid {albums} onOpen={onOpenAlbum} onMenu={onAlbumMenu} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .artists {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(200px, 280px) minmax(0, 1fr);
    border-top: 1px solid var(--line);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: var(--space-2) var(--space-2) var(--space-2) 0;
    overflow-y: auto;
    border-right: 1px solid var(--line);
  }
  .artist {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    text-align: left;
    font-weight: var(--weight-regular);
  }
  .artist[aria-current='true'] {
    background: var(--surface-selected);
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  .avatar {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: var(--radius-round);
    object-fit: cover;
    background: var(--art-empty);
    color: var(--text-muted);
  }
  .detail {
    min-width: 0;
    overflow-y: auto;
    padding: var(--space-5) 0 var(--space-6) var(--space-6);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-5);
    padding-bottom: var(--space-4);
    border-bottom: 1px solid var(--line);
  }
  .title {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  h2 {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
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
</style>

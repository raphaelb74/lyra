<script lang="ts">
  import AlbumGrid from './AlbumGrid.svelte';
  import Icon from './Icon.svelte';
  import { attempt, type Album, type Artist, type Backend } from './ipc';

  interface Props {
    api: Backend;
    /** Changes when the library changed; reloads the list. */
    version: number;
    onOpenAlbum: (album: Album) => void;
    onError: (message: string) => void;
  }

  const { api, version, onOpenAlbum, onError }: Props = $props();

  let artists = $state<Artist[]>([]);
  let selected = $state<Artist | null>(null);
  let albums = $state<Album[]>([]);

  /** Loads for library `forVersion`; a reply for an older one is dropped. */
  async function loadArtists(forVersion: number) {
    const list = await attempt(api.artists, onError);

    if (forVersion === version) artists = list ?? [];
  }

  async function open(artist: Artist) {
    selected = artist;
    albums = (await attempt(() => api.artistAlbums(artist.id), onError)) ?? [];
  }

  $effect(() => {
    void loadArtists(version);
  });
</script>

{#if selected}
  <button type="button" class="back" onclick={() => (selected = null)}
    ><Icon name="chevron-left" size={16} />All artists</button
  >
  <h3 class="artist">{selected.name}</h3>
  <AlbumGrid {albums} onOpen={onOpenAlbum} />
{:else if artists.length === 0}
  <p class="muted">No artists yet.</p>
{:else}
  <ul>
    {#each artists as artist (artist.id)}
      <li>
        <button
          type="button"
          onclick={() => {
            void open(artist);
          }}
        >
          <span class="truncate">{artist.name}</span>
          <small class="num"
            >{artist.albumCount}
            {artist.albumCount === 1 ? 'album' : 'albums'} · {artist.trackCount}
            {artist.trackCount === 1 ? 'track' : 'tracks'}</small
          >
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-0) var(--space-5);
  }
  li button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-3);
    text-align: left;
    border: 0;
    font-weight: var(--weight-regular);
  }
  small {
    color: var(--text-secondary);
    font-size: var(--type-caption);
    white-space: nowrap;
  }
  .artist {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
    margin: var(--space-3) 0 var(--space-5);
  }
</style>

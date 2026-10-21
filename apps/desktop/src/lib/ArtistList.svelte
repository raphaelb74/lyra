<script lang="ts">
  import AlbumGrid from './AlbumGrid.svelte';
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
    >← All artists</button
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
          <span>{artist.name}</span>
          <small
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
    gap: 6px 20px;
  }
  li button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 10px 12px;
  }
  li button:hover,
  li button:focus-visible {
    background: #182129;
  }
  li span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small,
  .muted {
    color: #a2adb7;
    font-size: 12px;
    white-space: nowrap;
  }
  .back {
    border: 0;
    background: transparent;
    padding: 6px 0;
    color: #a2adb7;
  }
  .artist {
    font-size: 24px;
    font-weight: 500;
    margin: 12px 0 20px;
  }
</style>

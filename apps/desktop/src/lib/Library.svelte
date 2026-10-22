<script lang="ts">
  import { onMount } from 'svelte';

  import AlbumGrid from './AlbumGrid.svelte';
  import AlbumView from './AlbumView.svelte';
  import ArtistList from './ArtistList.svelte';
  import TrackList from './TrackList.svelte';
  import { onLibraryChanged, onLibraryNotice, onScanStatus } from './events';
  import { scanText } from './format';
  import { attempt, type Album, type Backend, type ScanStatus } from './ipc';
  import { addToPlaylist, openMenu } from './menu.svelte';

  interface Props {
    /** Chosen in the sidebar. */
    section: 'albums' | 'artists' | 'tracks';
    api: Backend;
    /** Selected output device; empty when none. */
    device: string;
    /** Library id of the playing track. */
    playingId: number | null;
    onError: (message: string) => void;
  }

  const { section, api, device, playingId, onError }: Props = $props();

  let albums = $state<Album[]>([]);
  let hasFolders = $state(false);
  let loaded = $state(false);
  let scan = $state<ScanStatus | null>(null);
  /** A sentence about the last drop, e.g. a file instead of a folder. */
  let notice = $state<string | null>(null);
  /** The album opened, and the section it was opened from. */
  let opened = $state<{ album: Album; from: Props['section'] } | null>(null);

  /** Shown only in its own section: another sidebar choice leaves it. */
  const openAlbum = $derived(opened?.from === section ? opened.album : null);
  const titles = { albums: 'Albums', artists: 'Artists', tracks: 'Tracks' };

  /** Artist chosen in the Artists view; kept while an album is open. */
  let artistId = $state<number | null>(null);
  /** Bumped whenever the library changes, so every view reloads. */
  let version = $state(0);

  async function reload() {
    const both = await attempt(
      () => Promise.all([api.folders(), api.albums()]),
      onError,
    );

    if (both) {
      hasFolders = both[0].length > 0;
      albums = both[1];
    }

    // An open album may be gone or changed: keep it only if still there.
    const album = albums.find((a) => a.id === opened?.album.id);

    opened = album && opened ? { album, from: opened.from } : null;
    loaded = true;
    version += 1;
  }

  function showAlbum(album: Album) {
    opened = { album, from: section };
  }

  /** Opens an album by id, e.g. from a track's menu. */
  function showAlbumById(id: number) {
    const album = albums.find((candidate) => candidate.id === id);

    if (album) showAlbum(album);
  }

  async function albumTrackIds(album: Album): Promise<number[]> {
    return (await api.albumTracks(album.id)).map((track) => track.id);
  }

  function albumMenu(event: MouseEvent, album: Album) {
    openMenu(event, [
      {
        label: 'Play',
        icon: 'play',
        disabled: device === '',
        run: () =>
          void attempt(
            async () => api.playTracks(await albumTrackIds(album), 0, device),
            onError,
          ),
      },
      {
        label: 'Open album',
        icon: 'disc-3',
        run: () => {
          showAlbum(album);
        },
      },
      'separator',
      addToPlaylist(api, () => albumTrackIds(album), onError),
    ]);
  }

  onMount(() => {
    void reload();
    void attempt(api.scanStatus, onError).then((status) => {
      if (status) scan = status;
    });

    const unlisten = [
      onLibraryChanged(() => {
        notice = null;
        void reload();
      }),
      onLibraryNotice((message) => {
        notice = message;
      }),
      onScanStatus((status) => {
        scan = status;
      }),
    ];

    return () => {
      for (const pending of unlisten) {
        void pending.then((stop) => {
          stop();
        });
      }
    };
  });
</script>

{#if openAlbum}
  <AlbumView
    album={openAlbum}
    {api}
    {device}
    {playingId}
    {onError}
    onBack={() => (opened = null)}
  />
{:else}
  <section
    class="library"
    class:fill={section === 'artists'}
    aria-label={titles[section]}
  >
    <div class="heading">
      <h2>{titles[section]}</h2>
      {#if section === 'albums' && albums.length > 0}
        <span class="muted num">{albums.length} albums</span>
      {/if}
    </div>
    {#if notice !== null || scan?.running}
      <p class="muted scanning" role="status">
        {notice ?? (scan === null ? '' : scanText(scan))}
      </p>
    {/if}
    {#if albums.length === 0}
      {#if loaded}
        <div class="drop">
          {#if hasFolders}
            <p>No FLAC, WAV, DSF or DFF files found yet.</p>
          {:else}
            <p>Add a music folder, or drop one onto this window.</p>
            <button
              type="button"
              class="primary"
              onclick={() => void attempt(api.pickFolder, onError)}
              >Add music folder…</button
            >
          {/if}
        </div>
      {/if}
    {:else if section === 'albums'}
      <AlbumGrid {albums} onOpen={showAlbum} onMenu={albumMenu} />
    {:else if section === 'artists'}
      <ArtistList
        {api}
        {device}
        {version}
        selectedId={artistId}
        onSelect={(id: number) => (artistId = id)}
        onOpenAlbum={showAlbum}
        onAlbumMenu={albumMenu}
        {onError}
      />
    {:else}
      <TrackList
        {api}
        {device}
        {playingId}
        {version}
        onOpenAlbum={showAlbumById}
        {onError}
      />
    {/if}
  </section>
{/if}

<style>
  /* The Artists view's two panes fill the height and scroll on their own. */
  .fill {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .fill .heading {
    margin-bottom: var(--space-3);
  }
  .scanning {
    margin: 0 0 var(--space-4);
  }
  .drop {
    display: grid;
    justify-items: center;
    gap: var(--space-4);
  }
</style>

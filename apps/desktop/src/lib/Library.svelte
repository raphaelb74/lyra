<script lang="ts">
  import { onMount, tick } from 'svelte';

  import AlbumGrid from './AlbumGrid.svelte';
  import AlbumView from './AlbumView.svelte';
  import ArtistList from './ArtistList.svelte';
  import TrackList from './TrackList.svelte';
  import { onLibraryChanged, onLibraryNotice, onScanStatus } from './events';
  import FilterMenu from './FilterMenu.svelte';
  import {
    ALBUM_SORTS,
    ARTIST_SORTS,
    NO_ALBUM_FILTER,
    QUALITIES,
    chosen,
    filterAlbums,
    genresOf,
    type ArtistSort,
    type FilterGroup,
  } from './filters';
  import { scanText } from './format';
  import { attempt, type Album, type Backend, type ScanStatus } from './ipc';
  import { addToPlaylist, openMenu, queueActions } from './menu.svelte';

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

  /** Filters stay while an album is open and between sections. */
  let albumFilter = $state({ ...NO_ALBUM_FILTER });
  let artistQuery = $state('');
  let artistSort = $state<ArtistSort>('name');

  const shownAlbums = $derived(filterAlbums(albums, albumFilter));
  const ALL_GENRES = '';
  const albumGroups: FilterGroup[] = $derived([
    {
      title: 'Quality',
      options: QUALITIES,
      value: albumFilter.quality,
      onChoose: (value: string) => {
        albumFilter.quality = chosen(QUALITIES, value) ?? 'all';
      },
    },
    {
      title: 'Genre',
      options: [
        { value: ALL_GENRES, label: 'All genres' },
        ...genresOf(albums).map((genre) => ({ value: genre, label: genre })),
      ],
      value: albumFilter.genre ?? ALL_GENRES,
      onChoose: (value: string) => {
        albumFilter.genre = value === ALL_GENRES ? null : value;
      },
    },
    {
      title: 'Sort by',
      options: ALBUM_SORTS,
      value: albumFilter.sort,
      onChoose: (value: string) => {
        albumFilter.sort = chosen(ALBUM_SORTS, value) ?? 'artist';
      },
    },
  ]);
  const albumsFiltered = $derived(
    albumFilter.quality !== 'all' ||
      albumFilter.genre !== null ||
      albumFilter.sort !== 'artist',
  );
  const artistGroups: FilterGroup[] = $derived([
    {
      title: 'Sort by',
      options: ARTIST_SORTS,
      value: artistSort,
      onChoose: (value: string) => {
        artistSort = chosen(ARTIST_SORTS, value) ?? 'name';
      },
    },
  ]);

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

  /** Where the list was scrolled when an album opened; restored on Back. */
  let listScroll = 0;

  /** The app's scrolling area: the main view. */
  function scroller(): HTMLElement | null {
    return document.querySelector('main');
  }

  async function showAlbum(album: Album) {
    listScroll = scroller()?.scrollTop ?? 0;
    opened = { album, from: section };
    await tick();
    scroller()?.scrollTo({ top: 0 });
  }

  async function closeAlbum() {
    opened = null;
    await tick();
    scroller()?.scrollTo({ top: listScroll });
  }

  /** Opens an album by id, e.g. from a track's menu. */
  function showAlbumById(id: number) {
    const album = albums.find((candidate) => candidate.id === id);

    if (album) void showAlbum(album);
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
      ...queueActions(api, () => albumTrackIds(album), device, onError),
      {
        label: 'Open album',
        icon: 'disc-3',
        run: () => void showAlbum(album),
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
    onBack={closeAlbum}
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
        <div class="tools">
          <span class="muted num"
            >{shownAlbums.length === albums.length
              ? `${String(albums.length)} albums`
              : `${String(shownAlbums.length)} of ${String(albums.length)}`}</span
          >
          <input
            type="search"
            placeholder="Filter albums"
            aria-label="Filter albums by title, artist or year"
            bind:value={albumFilter.query}
          />
          <FilterMenu
            label="Filter and sort albums"
            groups={albumGroups}
            active={albumsFiltered}
            onReset={() => {
              albumFilter = { ...NO_ALBUM_FILTER, query: albumFilter.query };
            }}
          />
        </div>
      {:else if section === 'artists' && albums.length > 0}
        <div class="tools">
          <input
            type="search"
            placeholder="Filter artists"
            aria-label="Filter artists by name"
            bind:value={artistQuery}
          />
          <FilterMenu
            label="Sort artists"
            groups={artistGroups}
            active={artistSort !== 'name'}
            onReset={() => (artistSort = 'name')}
          />
        </div>
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
      {#if shownAlbums.length === 0}
        <p class="muted">
          No albums match.
          <button
            type="button"
            class="tertiary small"
            onclick={() => (albumFilter = { ...NO_ALBUM_FILTER })}
            >Clear filters</button
          >
        </p>
      {:else}
        <AlbumGrid albums={shownAlbums} onOpen={showAlbum} onMenu={albumMenu} />
      {/if}
    {:else if section === 'artists'}
      <ArtistList
        {api}
        {device}
        {version}
        query={artistQuery}
        sort={artistSort}
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
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .tools input {
    width: 220px;
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

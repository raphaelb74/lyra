<script lang="ts">
  import { onMount } from 'svelte';

  import AlbumGrid from './AlbumGrid.svelte';
  import AlbumView from './AlbumView.svelte';
  import ArtistList from './ArtistList.svelte';
  import TrackList from './TrackList.svelte';
  import { onLibraryChanged, onLibraryNotice, onScanStatus } from './events';
  import { scanText } from './format';
  import { attempt, type Album, type Backend, type ScanStatus } from './ipc';

  interface Props {
    api: Backend;
    /** Selected output device; empty when none. */
    device: string;
    /** Library id of the playing track. */
    playingId: number | null;
    onError: (message: string) => void;
  }

  const { api, device, playingId, onError }: Props = $props();

  let albums = $state<Album[]>([]);
  let hasFolders = $state(false);
  let loaded = $state(false);
  let scan = $state<ScanStatus | null>(null);
  /** A sentence about the last drop, e.g. a file instead of a folder. */
  let notice = $state<string | null>(null);
  let openAlbum = $state<Album | null>(null);
  type Tab = 'albums' | 'artists' | 'tracks';

  const tabs: readonly { id: Tab; label: string }[] = [
    { id: 'albums', label: 'Albums' },
    { id: 'artists', label: 'Artists' },
    { id: 'tracks', label: 'Tracks' },
  ];

  let tab = $state<Tab>('albums');
  /** Bumped whenever the library changes, so every tab reloads. */
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
    openAlbum = albums.find((a) => a.id === openAlbum?.id) ?? null;
    loaded = true;
    version += 1;
  }

  function showAlbum(album: Album) {
    openAlbum = album;
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
    onBack={() => (openAlbum = null)}
  />
{:else}
  <section aria-label="Library">
    <div class="tabs" role="tablist" aria-label="Browse by">
      {#each tabs as item (item.id)}
        <button
          type="button"
          role="tab"
          aria-selected={tab === item.id}
          onclick={() => (tab = item.id)}>{item.label}</button
        >
      {/each}
      <span class="muted num">{albums.length} albums</span>
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
    {:else if tab === 'albums'}
      <AlbumGrid {albums} onOpen={showAlbum} />
    {:else if tab === 'artists'}
      <ArtistList {api} {version} onOpenAlbum={showAlbum} {onError} />
    {:else}
      <TrackList {api} {device} {playingId} {version} {onError} />
    {/if}
  </section>
{/if}

<style>
  .tabs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-bottom: var(--space-5);
    border-bottom: 1px solid var(--line);
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--text-secondary);
  }
  .tabs button:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }
  .tabs [aria-selected='true'] {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .tabs .muted {
    margin-left: auto;
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

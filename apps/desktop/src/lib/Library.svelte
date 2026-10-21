<script lang="ts">
  import { onMount } from 'svelte';

  import AlbumGrid from './AlbumGrid.svelte';
  import AlbumView from './AlbumView.svelte';
  import ArtistList from './ArtistList.svelte';
  import TrackList from './TrackList.svelte';
  import {
    attempt,
    onLibraryChanged,
    onLibraryNotice,
    onScanStatus,
    type Album,
    type Backend,
    type Folder,
    type ScanStatus,
  } from './ipc';

  interface Props {
    api: Backend;
    /** Selected output device; empty when none. */
    device: string;
    /** Library id of the playing track. */
    playingId: number | null;
    onError: (message: string) => void;
  }

  const { api, device, playingId, onError }: Props = $props();

  let folders = $state<Folder[]>([]);
  let albums = $state<Album[]>([]);
  let loaded = $state(false);
  let scan = $state<ScanStatus | null>(null);
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
  /** Folder awaiting a second click to confirm removal. */
  let confirming = $state<number | null>(null);

  const scanText = $derived.by(() => {
    if (scan === null) return '';

    if (scan.running) {
      return scan.total === 0
        ? 'Looking for music…'
        : `Scanning ${String(scan.done)} of ${String(scan.total)} files…`;
    }

    if (scan.error !== null) return scan.error;

    const last = scan.last;

    if (last === null) return '';

    const failed =
      last.failed === 0 ? '' : ` · ${String(last.failed)} could not be read`;

    return `Up to date · ${String(last.added)} added, ${String(last.updated)} updated, ${String(last.removed)} removed${failed}`;
  });

  async function reload() {
    const both = await attempt(
      () => Promise.all([api.folders(), api.albums()]),
      onError,
    );

    if (both) [folders, albums] = both;

    // An open album may be gone or changed: keep it only if still there.
    openAlbum = albums.find((a) => a.id === openAlbum?.id) ?? null;
    loaded = true;
    version += 1;
  }

  function showAlbum(album: Album) {
    openAlbum = album;
  }

  function remove(folder: Folder) {
    if (confirming !== folder.id) {
      confirming = folder.id;

      return;
    }

    confirming = null;
    void attempt(() => api.removeFolder(folder.id), onError);
  }

  onMount(() => {
    void reload();
    void attempt(api.scanStatus, onError).then((status) => {
      if (status) scan = status;
    });

    const unlisten = [
      onLibraryChanged(() => {
        void reload();
      }),
      onScanStatus((status) => {
        scan = status;
      }),
      onLibraryNotice((message) => {
        notice = message;
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
  <section aria-labelledby="folders-title">
    <div class="heading">
      <h3 id="folders-title">Music folders</h3>
      <div class="actions">
        <button
          type="button"
          onclick={() => {
            notice = null;
            void attempt(api.pickFolder, onError);
          }}>Add folder…</button
        >
        <button
          type="button"
          disabled={folders.length === 0 || scan?.running === true}
          onclick={() => {
            void attempt(api.rescan, onError);
          }}>Rescan</button
        >
      </div>
    </div>
    {#if folders.length > 0}
      <ul class="folders">
        {#each folders as folder (folder.id)}
          <li>
            <span>{folder.path}</span>
            <button
              type="button"
              class:danger={confirming === folder.id}
              aria-label={confirming === folder.id
                ? `Confirm removing ${folder.path} from the library`
                : `Remove ${folder.path} from the library`}
              onclick={() => {
                remove(folder);
              }}
              onblur={() => {
                if (confirming === folder.id) confirming = null;
              }}
              >{confirming === folder.id
                ? 'Remove? Files stay'
                : 'Remove'}</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    <p class="muted" role="status" data-testid="scan-status">
      {notice ?? scanText}
    </p>
  </section>

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
      <span class="muted">{albums.length} albums</span>
    </div>
    {#if albums.length === 0}
      {#if loaded}
        <p class="drop">
          {folders.length === 0
            ? 'Add a music folder, or drop one onto this window.'
            : 'No FLAC, WAV, DSF or DFF files found yet.'}
        </p>
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
  section + section {
    margin-top: 32px;
  }
  .heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  .folders {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
    display: grid;
    gap: 6px;
  }
  .folders li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding: 8px 12px;
    background: #182129;
    border-radius: 8px;
    font-size: 13px;
  }
  .folders span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .folders button {
    padding: 4px 10px;
    font-size: 12px;
    white-space: nowrap;
  }
  .danger {
    border-color: #8a5a3c;
    color: #f0d2bf;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 20px;
    border-bottom: 1px solid #29343d;
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: transparent;
    padding: 10px 14px;
    color: #a2adb7;
  }
  .tabs [aria-selected='true'] {
    color: #d9eee4;
    border-bottom-color: #b4d9cb;
  }
  .tabs .muted {
    margin-left: auto;
  }
  .drop {
    border: 1px dashed #38534a;
    border-radius: 8px;
    padding: 28px;
    text-align: center;
    color: #a2adb7;
    font-size: 13px;
  }
</style>

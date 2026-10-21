<script lang="ts">
  import { onMount } from 'svelte';

  import AlbumView from './AlbumView.svelte';
  import { formatBadge } from './format';
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
    onError: (message: string) => void;
  }

  const { api, device, onError }: Props = $props();

  let folders = $state<Folder[]>([]);
  let albums = $state<Album[]>([]);
  let loaded = $state(false);
  let scan = $state<ScanStatus | null>(null);
  let notice = $state<string | null>(null);
  let openAlbumId = $state<number | null>(null);
  /** Folder awaiting a second click to confirm removal. */
  let confirming = $state<number | null>(null);

  const openAlbum = $derived(albums.find((a) => a.id === openAlbumId));
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

    loaded = true;
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
    {onError}
    onBack={() => (openAlbumId = null)}
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

  <section aria-labelledby="albums-title">
    <div class="heading">
      <h3 id="albums-title">Albums</h3>
      <span class="muted">{albums.length}</span>
    </div>
    {#if albums.length > 0}
      <ul class="grid">
        {#each albums as album (album.id)}
          <li>
            <button
              type="button"
              onclick={() => {
                openAlbumId = album.id;
              }}
            >
              {#if album.art}
                <img
                  src={album.art}
                  alt=""
                  loading="lazy"
                  width="200"
                  height="200"
                />
              {:else}
                <span class="placeholder" aria-hidden="true">♪</span>
              {/if}
              <strong>{album.title}</strong>
              <small>{album.artist}</small>
              <small class="badge">{formatBadge(album)}</small>
            </button>
          </li>
        {/each}
      </ul>
    {:else if loaded}
      <p class="drop">
        {folders.length === 0
          ? 'Add a music folder, or drop one onto this window.'
          : 'No FLAC, WAV, DSF or DFF files found yet.'}
      </p>
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
  .grid {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px 20px;
  }
  .grid button {
    width: 100%;
    display: grid;
    gap: 4px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 0;
  }
  .grid img,
  .placeholder {
    width: 100%;
    height: auto;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 8px;
    background: #182129;
    margin-bottom: 6px;
  }
  .placeholder {
    display: grid;
    place-items: center;
    font-size: 40px;
    color: #577369;
  }
  .grid button:focus-visible img,
  .grid button:focus-visible .placeholder,
  .grid button:hover img,
  .grid button:hover .placeholder {
    outline: 2px solid #577369;
    outline-offset: 2px;
  }
  .grid strong {
    font-weight: 500;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grid small {
    color: #a2adb7;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grid .badge {
    color: #b4d9cb;
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

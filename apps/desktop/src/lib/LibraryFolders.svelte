<script lang="ts">
  import { onMount } from 'svelte';

  import { onLibraryChanged, onLibraryNotice, onScanStatus } from './events';
  import { scanText } from './format';
  import { attempt, type Backend, type Folder, type ScanStatus } from './ipc';

  interface Props {
    api: Backend;
    onError: (message: string) => void;
  }

  const { api, onError }: Props = $props();

  let folders = $state<Folder[]>([]);
  let scan = $state<ScanStatus | null>(null);
  let notice = $state<string | null>(null);
  /** Folder awaiting a second click to confirm removal. */
  let confirming = $state<number | null>(null);

  async function load() {
    folders = (await attempt(api.folders, onError)) ?? folders;
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
    void load();
    void attempt(api.scanStatus, onError).then((status) => {
      if (status) scan = status;
    });

    const unlisten = [
      onLibraryChanged(() => {
        void load();
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

<div class="panel">
  <div class="setting">
    <div class="setting-text">
      <h4 id="folders-title">Music folders</h4>
      <p class="muted" role="status" data-testid="scan-status">
        {notice ??
          (scan === null || scanText(scan) === ''
            ? 'Lyra reads FLAC, WAV, DSF and DFF, and never changes your files.'
            : scanText(scan))}
      </p>
    </div>
    <div class="actions">
      <button
        type="button"
        class="small"
        disabled={folders.length === 0 || scan?.running === true}
        onclick={() => {
          void attempt(api.rescan, onError);
        }}>Rescan</button
      >
      <button
        type="button"
        class="small primary"
        onclick={() => {
          notice = null;
          void attempt(api.pickFolder, onError);
        }}>Add folder…</button
      >
    </div>
  </div>
  {#each folders as folder (folder.id)}
    <div class="setting folder">
      <span class="truncate">{folder.path}</span>
      <button
        type="button"
        class="small tertiary"
        class:danger={confirming === folder.id}
        aria-label={confirming === folder.id
          ? `Confirm removing ${folder.path} from the library`
          : `Remove ${folder.path} from the library`}
        onclick={() => {
          remove(folder);
        }}
        onblur={() => {
          if (confirming === folder.id) confirming = null;
        }}>{confirming === folder.id ? 'Remove? Files stay' : 'Remove'}</button
      >
    </div>
  {:else}
    <div class="setting">
      <p class="muted">
        No folders yet. Add one, or drop a folder onto the window.
      </p>
    </div>
  {/each}
</div>

<style>
  .folder {
    min-height: var(--row-height);
    padding: var(--space-1) 0;
    font-size: var(--type-small);
    color: var(--text-secondary);
  }
  .folder button {
    white-space: nowrap;
  }
</style>

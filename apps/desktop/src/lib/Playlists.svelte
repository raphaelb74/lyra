<script lang="ts">
  import { clock, formatBadge } from './format';
  import {
    attempt,
    type Backend,
    type Playlist,
    type PlaylistEntry,
  } from './ipc';

  interface Props {
    api: Backend;
    device: string;
    playingId: number | null;
    onError: (message: string) => void;
  }

  const { api, device, playingId, onError }: Props = $props();

  let playlists = $state<Playlist[]>([]);
  let openId = $state<number | null>(null);
  let entries = $state<PlaylistEntry[]>([]);
  let newName = $state('');
  let renaming = $state<string | null>(null);
  let confirmingDelete = $state(false);

  const open = $derived(playlists.find((p) => p.id === openId));

  async function loadPlaylists() {
    playlists = (await attempt(api.playlists, onError)) ?? [];
  }

  async function loadEntries(id: number) {
    entries = (await attempt(() => api.playlistEntries(id), onError)) ?? [];
  }

  /** Runs a change, then reloads what it touched. */
  async function change<T>(action: () => Promise<T>) {
    await attempt(action, onError);
    await loadPlaylists();

    if (openId !== null) await loadEntries(openId);
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();

    const id = await attempt(() => api.createPlaylist(newName), onError);

    newName = '';
    await loadPlaylists();

    if (id !== undefined) void show(id);
  }

  async function show(id: number) {
    openId = id;
    renaming = null;
    confirmingDelete = false;
    await loadEntries(id);
  }

  function play(index: number) {
    const ids = entries.map((entry) => entry.track.id);

    void attempt(() => api.playTracks(ids, index, device), onError);
  }

  async function rename(event: SubmitEvent, playlist: Playlist) {
    event.preventDefault();

    const name = renaming ?? '';

    renaming = null;
    await change(() => api.renamePlaylist(playlist.id, name));
  }

  async function remove(playlist: Playlist) {
    if (!confirmingDelete) {
      confirmingDelete = true;

      return;
    }

    confirmingDelete = false;
    openId = null;
    await change(() => api.deletePlaylist(playlist.id));
  }

  $effect(() => {
    void loadPlaylists();
  });
</script>

{#if open}
  <section aria-labelledby="playlist-title">
    <button type="button" class="back" onclick={() => (openId = null)}
      >← All playlists</button
    >
    <header>
      {#if renaming === null}
        <h3 id="playlist-title">{open.name}</h3>
        <p class="muted">
          {open.trackCount}
          {open.trackCount === 1 ? 'track' : 'tracks'} · {clock(
            open.durationSeconds,
          )}
        </p>
      {:else}
        <form
          onsubmit={(event) => {
            void rename(event, open);
          }}
        >
          <input
            aria-label="Playlist name"
            maxlength="200"
            bind:value={renaming}
          />
          <button type="submit">Save</button>
          <button type="button" onclick={() => (renaming = null)}>Cancel</button
          >
        </form>
      {/if}
      <div class="actions">
        <button
          type="button"
          disabled={entries.length === 0 || device === ''}
          onclick={() => {
            play(0);
          }}>Play</button
        >
        <button type="button" onclick={() => (renaming = open.name)}
          >Rename</button
        >
        <button
          type="button"
          class:danger={confirmingDelete}
          onblur={() => (confirmingDelete = false)}
          onclick={() => {
            void remove(open);
          }}>{confirmingDelete ? 'Delete? Tracks stay' : 'Delete'}</button
        >
      </div>
    </header>
    {#if entries.length === 0}
      <p class="muted">
        Empty. Add tracks with the + buttons in albums and the track list.
      </p>
    {:else}
      <ol>
        {#each entries as entry, index (entry.entryId)}
          <li>
            <button
              type="button"
              class="row"
              aria-current={entry.track.id === playingId ? 'true' : undefined}
              disabled={device === ''}
              onclick={() => {
                play(index);
              }}
            >
              <span class="number">{index + 1}</span>
              <span class="title"
                >{entry.track.title}
                <small>{entry.track.artist} · {entry.track.album}</small></span
              >
              <span class="secondary">{formatBadge(entry.track)}</span>
              <span class="secondary time"
                >{entry.track.durationSeconds === null
                  ? ''
                  : clock(entry.track.durationSeconds)}</span
              >
            </button>
            <button
              type="button"
              class="tool"
              aria-label={`Move ${entry.track.title} up`}
              disabled={index === 0}
              onclick={() => {
                void change(() => api.moveInPlaylist(entry.entryId, index - 1));
              }}>↑</button
            >
            <button
              type="button"
              class="tool"
              aria-label={`Move ${entry.track.title} down`}
              disabled={index === entries.length - 1}
              onclick={() => {
                void change(() => api.moveInPlaylist(entry.entryId, index + 1));
              }}>↓</button
            >
            <button
              type="button"
              class="tool"
              aria-label={`Remove ${entry.track.title} from the playlist`}
              onclick={() => {
                void change(() => api.removeFromPlaylist(entry.entryId));
              }}>✕</button
            >
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{:else}
  <section aria-labelledby="playlists-title">
    <div class="heading">
      <h3 id="playlists-title">Playlists</h3>
      <form onsubmit={create}>
        <input
          aria-label="New playlist name"
          placeholder="New playlist"
          maxlength="200"
          bind:value={newName}
        />
        <button type="submit" disabled={newName.trim() === ''}>Create</button>
      </form>
    </div>
    {#if playlists.length === 0}
      <p class="muted">No playlists yet. Create one, then add tracks to it.</p>
    {:else}
      <ul>
        {#each playlists as playlist (playlist.id)}
          <li>
            <button
              type="button"
              onclick={() => {
                void show(playlist.id);
              }}
            >
              <span>{playlist.name}</span>
              <small
                >{playlist.trackCount}
                {playlist.trackCount === 1 ? 'track' : 'tracks'} · {clock(
                  playlist.durationSeconds,
                )}</small
              >
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .heading,
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 16px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  header h3 {
    font-size: 24px;
  }
  .muted,
  small {
    color: var(--text-muted);
    font-size: 13px;
  }
  form,
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  input {
    font: inherit;
    font-size: 13px;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    padding: 7px 10px;
  }
  ul,
  ol {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 2px;
  }
  ul button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 10px 12px;
  }
  ul button:hover,
  ul button:focus-visible {
    background: var(--surface);
  }
  ol li {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .row {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: 3em minmax(0, 1fr) 6em 4em;
    gap: 16px;
    align-items: center;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 9px 12px;
  }
  .row:hover:not(:disabled),
  .row:focus-visible {
    background: var(--surface);
  }
  .row[aria-current='true'] {
    background: var(--surface-current);
    color: var(--text-current);
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title small {
    margin-left: 8px;
    font-size: 12px;
  }
  .number,
  .secondary {
    color: var(--text-muted);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .time {
    text-align: right;
  }
  .tool {
    border: 0;
    background: transparent;
    padding: 6px 8px;
    color: var(--text-muted);
  }
  .tool:hover:not(:disabled),
  .tool:focus-visible {
    color: var(--text-current);
    background: var(--surface);
  }
</style>

<script lang="ts">
  import { clock, formatBadge } from './format';
  import Icon from './Icon.svelte';
  import { addToPlaylist, openMenu, queueActions } from './menu.svelte';
  import { rowEvents } from './rows';
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

  /** The row a click selected; a double click plays. */
  let selected = $state<number | null>(null);

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
      ><Icon name="chevron-left" size={16} />All playlists</button
    >
    <header class="heading">
      {#if renaming === null}
        <h3 id="playlist-title">{open.name}</h3>
        <p class="muted num">
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
          class="primary"
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
      <ol class="rows">
        {#each entries as entry, index (entry.entryId)}
          <li>
            <button
              type="button"
              class="row"
              aria-current={entry.track.id === playingId ? 'true' : undefined}
              disabled={device === ''}
              data-selected={selected === entry.entryId}
              {...rowEvents(
                () => (selected = entry.entryId),
                () => {
                  play(index);
                },
              )}
              oncontextmenu={(event) => {
                openMenu(event, [
                  {
                    label: 'Play',
                    icon: 'play',
                    disabled: device === '',
                    run: () => {
                      play(index);
                    },
                  },
                  ...queueActions(
                    api,
                    () => Promise.resolve([entry.track.id]),
                    device,
                    onError,
                  ),
                  addToPlaylist(
                    api,
                    () => Promise.resolve([entry.track.id]),
                    onError,
                  ),
                  'separator',
                  {
                    label: 'Remove from playlist',
                    icon: 'x',
                    danger: true,
                    run: () =>
                      void change(() => api.removeFromPlaylist(entry.entryId)),
                  },
                ]);
              }}
            >
              <span class="meta end">{index + 1}</span>
              <span class="title"
                >{entry.track.title}
                <small>{entry.track.artist} · {entry.track.album}</small></span
              >
              <span class="meta">{formatBadge(entry.track)}</span>
              <span class="meta end"
                >{entry.track.durationSeconds === null
                  ? ''
                  : clock(entry.track.durationSeconds)}</span
              >
            </button>
            <button
              type="button"
              class="icon"
              aria-label={`Move ${entry.track.title} up`}
              disabled={index === 0}
              onclick={() => {
                void change(() => api.moveInPlaylist(entry.entryId, index - 1));
              }}><Icon name="arrow-up" size={16} /></button
            >
            <button
              type="button"
              class="icon"
              aria-label={`Move ${entry.track.title} down`}
              disabled={index === entries.length - 1}
              onclick={() => {
                void change(() => api.moveInPlaylist(entry.entryId, index + 1));
              }}><Icon name="arrow-down" size={16} /></button
            >
            <button
              type="button"
              class="icon"
              aria-label={`Remove ${entry.track.title} from the playlist`}
              onclick={() => {
                void change(() => api.removeFromPlaylist(entry.entryId));
              }}><Icon name="x" size={16} /></button
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
      <ul class="list">
        {#each playlists as playlist (playlist.id)}
          <li>
            <button
              type="button"
              onclick={() => {
                void show(playlist.id);
              }}
            >
              <span>{playlist.name}</span>
              <small class="num"
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
  header h3 {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
  }
  form {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .rows {
    --columns: 3em minmax(0, 1fr) 6em 4em;
  }
  .list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: var(--space-0);
  }
  .list button {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
    text-align: left;
    border: 0;
    font-weight: var(--weight-regular);
  }
  .list small {
    color: var(--text-secondary);
    font-size: var(--type-small);
  }
</style>

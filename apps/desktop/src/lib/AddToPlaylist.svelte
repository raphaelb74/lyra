<script lang="ts">
  import Icon from './Icon.svelte';
  import { attempt, type Backend, type Playlist } from './ipc';
  import Popover from './Popover.svelte';

  interface Props {
    api: Backend;
    trackIds: number[];
    /** Accessible name of the button, e.g. "Add Aladdin Sane to a playlist". */
    label: string;
    /** Visible text, e.g. "Add to playlist". */
    text: string;
    onError: (message: string) => void;
  }

  const { api, trackIds, label, text, onError }: Props = $props();

  let playlists = $state<Playlist[]>([]);
  let name = $state('');
  let added = $state<string | null>(null);

  async function load() {
    added = null;
    playlists = (await attempt(api.playlists, onError)) ?? [];
  }

  async function addTo(playlist: Playlist, close: () => void) {
    const done = await attempt(
      () => api.addToPlaylist(playlist.id, trackIds),
      onError,
    );

    if (done !== undefined) {
      added = `Added to ${playlist.name}`;
      close();
    }
  }

  async function create(event: SubmitEvent, close: () => void) {
    event.preventDefault();

    const id = await attempt(() => api.createPlaylist(name), onError);

    if (id === undefined) return;

    const playlist = {
      id,
      name: name.trim(),
      trackCount: 0,
      durationSeconds: 0,
    };

    name = '';
    await addTo(playlist, close);
  }
</script>

<div class="add">
  <Popover {label} triggerClass="with-icon" onOpen={load}>
    {#snippet trigger()}
      <Icon name="plus" size={16} /><span>{text}</span>
    {/snippet}
    {#snippet children(close: () => void)}
      <div class="items">
        {#each playlists as playlist (playlist.id)}
          <button
            type="button"
            onclick={() => {
              void addTo(playlist, close);
            }}>{playlist.name}</button
          >
        {/each}
      </div>
      <form
        onsubmit={(event) => {
          void create(event, close);
        }}
      >
        <input
          aria-label="New playlist name"
          placeholder="New playlist"
          maxlength="200"
          bind:value={name}
        />
        <button
          type="submit"
          class="primary small"
          disabled={name.trim() === ''}>Create</button
        >
      </form>
    {/snippet}
  </Popover>
  {#if added}
    <span class="confirm" role="status">{added}</span>
  {/if}
</div>

<style>
  .add {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .confirm {
    white-space: nowrap;
  }
  .items {
    display: grid;
    gap: var(--space-0);
  }
  .items > button {
    text-align: left;
    border: 0;
    padding: var(--space-2) var(--space-3);
    font-size: var(--type-small);
    font-weight: var(--weight-regular);
  }
  form {
    display: flex;
    gap: var(--space-2);
  }
  .items:not(:empty) + form {
    padding-top: var(--space-2);
    border-top: 1px solid var(--line);
    margin-top: var(--space-1);
  }
  input {
    flex: 1;
    min-width: 0;
  }
</style>

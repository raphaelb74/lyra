<script lang="ts">
  import { attempt, type Backend, type Playlist } from './ipc';

  interface Props {
    api: Backend;
    trackIds: number[];
    /** Accessible name of the button, e.g. "Add Aladdin Sane to a playlist". */
    label: string;
    /** Visible text; a "+" when omitted. */
    text?: string;
    onError: (message: string) => void;
  }

  const { api, trackIds, label, text, onError }: Props = $props();

  let open = $state(false);
  let playlists = $state<Playlist[]>([]);
  let name = $state('');
  let added = $state<string | null>(null);
  let container = $state<HTMLDivElement | undefined>();

  async function toggle() {
    open = !open;
    added = null;

    if (open) playlists = (await attempt(api.playlists, onError)) ?? [];
  }

  async function addTo(playlist: Playlist) {
    const done = await attempt(
      () => api.addToPlaylist(playlist.id, trackIds),
      onError,
    );

    if (done !== undefined) {
      added = `Added to ${playlist.name}`;
      open = false;
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();

    const id = await attempt(() => api.createPlaylist(name), onError);

    if (id === undefined) return;

    await addTo({ id, name: name.trim(), trackCount: 0, durationSeconds: 0 });
    name = '';
  }

  /** Closes the menu when a press lands outside it. */
  function closeOutside(event: PointerEvent) {
    const target = event.target;

    if (open && !(target instanceof Node && container?.contains(target))) {
      open = false;
    }
  }

  function closeOnEscape(event: KeyboardEvent) {
    if (open && event.key === 'Escape') open = false;
  }
</script>

<svelte:document onpointerdown={closeOutside} onkeydown={closeOnEscape} />

<div class="add" bind:this={container}>
  <button
    type="button"
    class="trigger"
    aria-label={label}
    aria-expanded={open}
    title={label}
    onclick={() => {
      void toggle();
    }}>{text ?? '+'}</button
  >
  {#if added}
    <span class="added" role="status">{added}</span>
  {/if}
  {#if open}
    <div class="menu">
      {#each playlists as playlist (playlist.id)}
        <button
          type="button"
          onclick={() => {
            void addTo(playlist);
          }}>{playlist.name}</button
        >
      {/each}
      <form onsubmit={create}>
        <input
          aria-label="New playlist name"
          placeholder="New playlist"
          maxlength="200"
          bind:value={name}
        />
        <button type="submit" disabled={name.trim() === ''}>Create</button>
      </form>
    </div>
  {/if}
</div>

<style>
  .add {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .trigger {
    padding: 4px 10px;
    font-size: 13px;
    border-color: var(--border);
    background: transparent;
    white-space: nowrap;
  }
  .added {
    color: var(--accent);
    font-size: 12px;
    white-space: nowrap;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 10;
    min-width: 220px;
    max-height: 320px;
    overflow-y: auto;
    display: grid;
    gap: 2px;
    padding: 6px;
    background: var(--surface-raised);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 40%);
  }
  .menu > button {
    text-align: left;
    border: 0;
    background: transparent;
    padding: 8px 10px;
    font-size: 13px;
  }
  .menu > button:hover,
  .menu > button:focus-visible {
    background: var(--surface-selected);
  }
  form {
    display: flex;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: 13px;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 6px 8px;
  }
  form button {
    padding: 4px 10px;
    font-size: 12px;
  }
</style>

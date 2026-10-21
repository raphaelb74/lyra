<script lang="ts">
  import Icon from './Icon.svelte';
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
    class={text === undefined ? 'icon' : 'small'}
    aria-label={label}
    aria-expanded={open}
    title={label}
    onclick={() => {
      void toggle();
    }}
    >{#if text === undefined}<Icon
        name="plus"
        size={16}
      />{:else}{text}{/if}</button
  >
  {#if added}
    <span class="confirm" role="status">{added}</span>
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
        <button type="submit" class="small" disabled={name.trim() === ''}
          >Create</button
        >
      </form>
    </div>
  {/if}
</div>

<style>
  .add {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .confirm {
    white-space: nowrap;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + var(--space-1));
    z-index: 10;
    min-width: 220px;
    max-height: 320px;
    overflow-y: auto;
    display: grid;
    gap: var(--space-0);
    padding: var(--space-1);
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-pop);
  }
  .menu > button {
    text-align: left;
    border: 0;
    padding: var(--space-2) var(--space-3);
    font-size: var(--type-small);
    font-weight: var(--weight-regular);
  }
  form {
    display: flex;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--line);
    margin-top: var(--space-1);
  }
  input {
    flex: 1;
    min-width: 0;
  }
</style>

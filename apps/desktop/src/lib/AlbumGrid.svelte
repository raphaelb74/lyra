<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Album } from './ipc';

  interface Props {
    albums: Album[];
    onOpen: (album: Album) => void;
    /** Right click (or the Menu key) on an album. */
    onMenu?: (event: MouseEvent, album: Album) => void;
  }

  const { albums, onOpen, onMenu }: Props = $props();
</script>

<ul class="grid">
  {#each albums as album (album.id)}
    <li>
      <button
        type="button"
        onclick={() => {
          onOpen(album);
        }}
        oncontextmenu={(event) => {
          onMenu?.(event, album);
        }}
      >
        {#if album.thumb}
          <img
            class="art"
            src={album.thumb}
            alt=""
            loading="lazy"
            decoding="async"
            width="200"
            height="200"
          />
        {:else}
          <span class="art" aria-hidden="true"
            ><Icon name="disc-3" size={40} /></span
          >
        {/if}
        <strong class="truncate">{album.title}</strong>
        <small class="truncate"
          >{album.artist}{album.year === null
            ? ''
            : ` · ${String(album.year)}`}</small
        >
      </button>
    </li>
  {/each}
</ul>

<style>
  .grid {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));
    gap: var(--space-5);
  }
  button {
    width: 100%;
    display: grid;
    gap: var(--space-0);
    text-align: left;
    border: 0;
    padding: 0;
    font-weight: var(--weight-regular);
  }
  button:hover:not(:disabled) {
    background: transparent;
  }
  .art {
    width: 100%;
    height: auto;
    margin-bottom: var(--space-2);
    transition: opacity var(--dur-fast) var(--ease);
  }
  button:hover .art {
    opacity: 0.85;
  }
  button:focus-visible {
    outline: none;
  }
  button:focus-visible .art {
    outline: var(--focus-ring);
    outline-offset: 2px;
  }
  strong {
    font-weight: var(--weight-medium);
  }
  small {
    color: var(--text-secondary);
    font-size: var(--type-small);
    line-height: var(--leading-small);
  }
</style>

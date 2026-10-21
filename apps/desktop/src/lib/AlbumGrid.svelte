<script lang="ts">
  import { formatBadge } from './format';
  import type { Album } from './ipc';

  interface Props {
    albums: Album[];
    onOpen: (album: Album) => void;
  }

  const { albums, onOpen }: Props = $props();
</script>

<ul class="grid">
  {#each albums as album (album.id)}
    <li>
      <button
        type="button"
        onclick={() => {
          onOpen(album);
        }}
      >
        {#if album.art}
          <img src={album.art} alt="" loading="lazy" width="200" height="200" />
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

<style>
  .grid {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px 20px;
  }
  button {
    width: 100%;
    display: grid;
    gap: 4px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 0;
  }
  img,
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
  button:focus-visible img,
  button:focus-visible .placeholder,
  button:hover img,
  button:hover .placeholder {
    outline: 2px solid #577369;
    outline-offset: 2px;
  }
  strong {
    font-weight: 500;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small {
    color: #a2adb7;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    color: #b4d9cb;
  }
</style>

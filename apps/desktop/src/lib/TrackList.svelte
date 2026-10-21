<script lang="ts">
  import AddToPlaylist from './AddToPlaylist.svelte';
  import { clock, formatBadge } from './format';
  import { attempt, type Backend, type LibraryTrack } from './ipc';

  interface Props {
    api: Backend;
    device: string;
    playingId: number | null;
    /** Changes when the library changed; reloads the list. */
    version: number;
    onError: (message: string) => void;
  }

  const { api, device, playingId, version, onError }: Props = $props();

  /** Rows rendered at once; refine the search to see others. */
  const SHOWN = 500;

  let tracks = $state<LibraryTrack[]>([]);
  let search = $state('');

  /** Lowercase search text per track, built once per load. */
  const searchable = $derived(
    tracks.map((track) =>
      `${track.title} ${track.artist} ${track.album}`.toLowerCase(),
    ),
  );
  const matches = $derived.by(() => {
    const words = search.toLowerCase().split(/\s+/).filter(Boolean);

    if (words.length === 0) return tracks;

    return tracks.filter((_, index) =>
      words.every((word) => searchable[index]?.includes(word)),
    );
  });
  const shown = $derived(matches.slice(0, SHOWN));

  /** Loads for library `forVersion`; a reply for an older one is dropped. */
  async function load(forVersion: number) {
    const list = await attempt(api.tracks, onError);

    if (forVersion === version) tracks = list ?? [];
  }

  $effect(() => {
    void load(version);
  });

  /** Plays the shown list from `index`. */
  function play(index: number) {
    const ids = shown.map((t) => t.id);

    void attempt(() => api.playTracks(ids, index, device), onError);
  }
</script>

<div class="heading">
  <input
    type="search"
    placeholder="Search title, artist or album"
    aria-label="Search tracks"
    bind:value={search}
  />
  <span class="muted num"
    >{matches.length === tracks.length
      ? `${String(tracks.length)} tracks`
      : `${String(matches.length)} of ${String(tracks.length)}`}</span
  >
</div>
<ol class="rows">
  {#each shown as track, index (track.id)}
    <li>
      <button
        type="button"
        class="row"
        aria-current={track.id === playingId ? 'true' : undefined}
        disabled={device === ''}
        onclick={() => {
          play(index);
        }}
      >
        <span class="title">{track.title}</span>
        <span class="meta">{track.artist}</span>
        <span class="meta">{track.album}</span>
        <span class="meta">{formatBadge(track)}</span>
        <span class="meta end"
          >{track.durationSeconds === null
            ? ''
            : clock(track.durationSeconds)}</span
        >
      </button>
      <AddToPlaylist
        {api}
        trackIds={[track.id]}
        label={`Add ${track.title} to a playlist`}
        {onError}
      />
    </li>
  {/each}
</ol>
{#if matches.length > SHOWN}
  <p class="muted">
    Showing the first {SHOWN}. Refine the search to see the others.
  </p>
{/if}

<style>
  input {
    width: min(100%, 360px);
  }
  .heading {
    justify-content: flex-start;
  }
  .rows {
    --columns: minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr) 6em 4em;
  }
  @media (max-width: 950px) {
    .rows {
      --columns: minmax(0, 2fr) minmax(0, 1fr) 4em;
    }
    .row span:nth-child(3),
    .row span:nth-child(4) {
      display: none;
    }
  }
</style>

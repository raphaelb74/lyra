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

  const matches = $derived.by(() => {
    const words = search.toLowerCase().split(/\s+/).filter(Boolean);

    if (words.length === 0) return tracks;

    return tracks.filter((track) => {
      const text =
        `${track.title} ${track.artist} ${track.album}`.toLowerCase();

      return words.every((word) => text.includes(word));
    });
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

<div class="bar">
  <input
    type="search"
    placeholder="Search title, artist or album"
    aria-label="Search tracks"
    bind:value={search}
  />
  <span class="muted"
    >{matches.length === tracks.length
      ? `${String(tracks.length)} tracks`
      : `${String(matches.length)} of ${String(tracks.length)}`}</span
  >
</div>
<ol>
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
        <span class="secondary">{track.artist}</span>
        <span class="secondary">{track.album}</span>
        <span class="secondary">{formatBadge(track)}</span>
        <span class="secondary time"
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
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 16px;
  }
  input {
    font: inherit;
    font-size: 13px;
    color: inherit;
    background: #182129;
    border: 1px solid #38534a;
    border-radius: 8px;
    padding: 8px 12px;
    width: min(100%, 360px);
  }
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  ol {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 2px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .row {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr) 6em 4em;
    gap: 16px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 9px 12px;
  }
  .row:hover:not(:disabled),
  .row:focus-visible {
    background: #182129;
  }
  .row[aria-current='true'] {
    background: #1f2d2a;
    color: #d9eee4;
  }
  .row span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .secondary {
    color: #a2adb7;
    font-size: 13px;
  }
  .time {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  @media (max-width: 950px) {
    .row {
      grid-template-columns: minmax(0, 2fr) minmax(0, 1fr) 4em;
    }
    .row span:nth-child(3),
    .row span:nth-child(4) {
      display: none;
    }
  }
</style>

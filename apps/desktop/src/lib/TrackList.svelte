<script lang="ts">
  import { sortTracks, type TrackColumn, type TrackSort } from './filters';
  import { clock, formatBadge } from './format';
  import Icon from './Icon.svelte';
  import { attempt, type Backend, type LibraryTrack } from './ipc';
  import {
    addToPlaylist,
    openMenu,
    playShuffled,
    queueActions,
    type MenuItem,
  } from './menu.svelte';
  import RowMenu from './RowMenu.svelte';
  import { rowEvents } from './rows';

  interface Props {
    api: Backend;
    device: string;
    playingId: number | null;
    /** Changes when the library changed; reloads the list. */
    version: number;
    onOpenAlbum: (id: number) => void;
    onError: (message: string) => void;
  }

  const { api, device, playingId, version, onOpenAlbum, onError }: Props =
    $props();

  /** The row a click selected; a double click plays. */
  let selected = $state<number | null>(null);

  /** Rows rendered at once; refine the search to see others. */
  const SHOWN = 500;

  const COLUMNS: readonly { id: TrackColumn; label: string }[] = [
    { id: 'title', label: 'Title' },
    { id: 'artist', label: 'Artist' },
    { id: 'album', label: 'Album' },
    { id: 'format', label: 'Format' },
    { id: 'length', label: 'Length' },
  ];

  let tracks = $state<LibraryTrack[]>([]);
  let search = $state('');
  /** Null: the library order (artist, album, track). */
  let sort = $state<TrackSort | null>(null);

  /** A header click sorts by that column; again, the other way; a third time, back to the library order. */
  function sortBy(column: TrackColumn) {
    if (sort?.column !== column) sort = { column, descending: false };
    else if (!sort.descending) sort = { column, descending: true };
    else sort = null;
  }

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
  const sorted = $derived(sort === null ? matches : sortTracks(matches, sort));
  const shown = $derived(sorted.slice(0, SHOWN));

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

  /** Every matching track, in the order shown, beyond the rows drawn. */
  function playAll(shuffled: boolean) {
    const ids = sorted.map((t) => t.id);

    void attempt(
      () =>
        shuffled
          ? playShuffled(api, ids, device)
          : api.playTracks(ids, 0, device),
      onError,
    );
  }

  /** The row's menu: on right-click and behind its ⋯. */
  function trackMenu(track: LibraryTrack, index: number): MenuItem[] {
    return [
      {
        label: 'Play',
        icon: 'play',
        disabled: device === '',
        run: () => {
          play(index);
        },
      },
      ...queueActions(api, () => Promise.resolve([track.id]), device, onError),
      {
        label: 'Go to album',
        icon: 'disc-3',
        run: () => {
          onOpenAlbum(track.albumId);
        },
      },
      'separator',
      addToPlaylist(api, () => Promise.resolve([track.id]), onError),
    ];
  }
</script>

<div class="heading">
  <h2>Tracks</h2>
  <div class="actions">
    <span class="muted num"
      >{matches.length === tracks.length
        ? `${String(tracks.length)} tracks`
        : `${String(matches.length)} of ${String(tracks.length)}`}</span
    >
    <input
      type="search"
      placeholder="Search title, artist or album"
      aria-label="Search tracks"
      bind:value={search}
    />
    <button
      type="button"
      class="with-icon"
      disabled={sorted.length === 0 || device === ''}
      onclick={() => {
        playAll(true);
      }}><Icon name="shuffle" size={16} />Shuffle</button
    >
    <button
      type="button"
      class="primary with-icon"
      disabled={sorted.length === 0 || device === ''}
      onclick={() => {
        playAll(false);
      }}><Icon name="play" size={16} />Play</button
    >
  </div>
</div>
<div class="table">
  <div class="columns">
    <div class="head">
      {#each COLUMNS as column (column.id)}
        {@const active = sort?.column === column.id}
        <button
          type="button"
          class="sort"
          class:end={column.id === 'length'}
          class:active
          aria-label={`Sort by ${column.label}${active ? (sort?.descending ? ', descending' : ', ascending') : ''}`}
          onclick={() => {
            sortBy(column.id);
          }}
          >{column.label}{#if active}<Icon
              name={sort?.descending ? 'arrow-down' : 'arrow-up'}
              size={12}
            />{/if}</button
        >
      {/each}
    </div>
    <span class="spacer"></span>
  </div>
  <ol class="rows">
    {#each shown as track, index (track.id)}
      <li>
        <button
          type="button"
          class="row"
          aria-current={track.id === playingId ? 'true' : undefined}
          disabled={device === ''}
          data-selected={selected === track.id}
          {...rowEvents(
            () => (selected = track.id),
            () => {
              play(index);
            },
          )}
          oncontextmenu={(event) => {
            openMenu(event, trackMenu(track, index));
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
        <RowMenu
          label={`More for ${track.title}`}
          items={() => trackMenu(track, index)}
        />
      </li>
    {/each}
  </ol>
</div>
{#if matches.length > SHOWN}
  <p class="muted">
    First {SHOWN} shown. Refine the search.
  </p>
{/if}

<style>
  input {
    width: 260px;
  }
  .heading .actions {
    gap: var(--space-3);
  }
  .table {
    --columns: minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr) 6em 4em;
  }
  /* The header stays in view and lines up with the rows below. */
  .columns {
    position: sticky;
    /* Up to the window's edge: over the scroll area's top padding, so no
       row shows above the header (padding and margin keep its place). */
    top: calc(-1 * var(--space-2));
    padding-top: var(--space-2);
    margin-top: calc(-1 * var(--space-2));
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--bg);
    border-bottom: 1px solid var(--line);
    margin-bottom: var(--space-1);
  }
  .head {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: var(--columns);
    gap: var(--space-4);
    padding: 0 var(--space-3);
  }
  .spacer {
    width: var(--hit);
    flex-shrink: 0;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    padding: var(--space-2) 0;
    border: 0;
    border-radius: 0;
    color: var(--text-muted);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    font-weight: var(--weight-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .sort:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }
  .sort.active {
    color: var(--text);
  }
  .sort.end {
    justify-content: flex-end;
  }
  @media (max-width: 950px) {
    .table {
      --columns: minmax(0, 2fr) minmax(0, 1fr) 4em;
    }
    .row span:nth-child(3),
    .row span:nth-child(4),
    .sort:nth-child(3),
    .sort:nth-child(4) {
      display: none;
    }
  }
</style>

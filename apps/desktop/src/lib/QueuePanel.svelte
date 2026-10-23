<script lang="ts">
  import { clock } from './format';
  import Icon from './Icon.svelte';
  import {
    attempt,
    type Backend,
    type PlayerStatus,
    type QueueEntry,
  } from './ipc';
  import { openMenu } from './menu.svelte';
  import { DRAG_THRESHOLD, dropAfter, gapAt } from './reorder';
  import { rowEvents } from './rows';
  import { onMount, tick } from 'svelte';

  import { onLibraryChanged } from './events';

  interface Props {
    api: Backend;
    status: PlayerStatus;
    /** Selected output device; plays a stopped queue. */
    device: string;
    onClose: () => void;
    onError: (message: string) => void;
  }

  const { api, status, device, onClose, onError }: Props = $props();

  let entries = $state<QueueEntry[]>([]);
  /** The row a click selected; a double click plays. */
  let selected = $state<number | null>(null);
  let scroller = $state<HTMLElement>();
  let upNextList = $state<HTMLOListElement>();
  /** A press on an Up next row; a drag once it moves (`gap` set). */
  let drag = $state<{
    from: number;
    pointer: number;
    startY: number;
    gap: number | null;
  } | null>(null);
  /** The Up next row to focus once the queue reloads after a keyboard move. */
  let refocus: number | null = null;

  const current = $derived(status.track?.queueIndex ?? null);
  /** Changes only when the queue does, not with every status event. */
  const revision = $derived(status.queueRevision);
  const playing = $derived(current === null ? undefined : entries[current]);
  /** Queue positions in the order they will play after the current track. */
  const upNext = $derived.by(() => {
    const positions = entries.map((_, index) => index);

    if (current === null) return positions;

    const after = positions.slice(current + 1);

    return status.repeat === 'all'
      ? [...after, ...positions.slice(0, current)]
      : after;
  });

  /** Loads queue `forRevision`; a reply for an older one is dropped. */
  async function load(forRevision: number) {
    const list = await attempt(api.queue, onError);

    if (forRevision !== revision) return;

    entries = list ?? [];

    if (refocus === null) return;

    const row = refocus;

    refocus = null;
    await tick();
    upNextList?.querySelectorAll<HTMLButtonElement>('.entry')[row]?.focus();
  }

  $effect(() => {
    void load(revision);
  });

  // An edit may rename queued tracks.
  onMount(() => {
    const unlisten = onLibraryChanged(() => {
      void load(revision);
    });

    return () => {
      void unlisten.then((stop) => {
        stop();
      });
    };
  });

  /** Plays entry `index`; a stopped queue starts again from it. */
  function play(index: number) {
    void attempt(() => api.skipTo(index, device), onError);
  }

  function moveNext(index: number) {
    void attempt(() => api.queueMove(index, null), onError);
  }

  /** Moves Up next row `from` into `gap`. */
  function moveTo(from: number, gap: number) {
    const index = upNext[from];
    const after = dropAfter(upNext, from, gap);

    if (index === undefined || after === undefined) return false;

    void attempt(() => api.queueMove(index, after), onError);

    return true;
  }

  function press(event: PointerEvent, from: number) {
    if (event.button !== 0) return;

    drag = { from, pointer: event.pointerId, startY: event.clientY, gap: null };
  }

  function dragMove(event: PointerEvent & { currentTarget: HTMLElement }) {
    if (drag?.pointer !== event.pointerId) return;

    if (drag.gap === null) {
      if (Math.abs(event.clientY - drag.startY) < DRAG_THRESHOLD) return;

      event.currentTarget.setPointerCapture(event.pointerId);
    }

    // Near the pane's top or bottom edge, scroll to reach further rows.
    const edge = scroller?.getBoundingClientRect();

    if (edge && event.clientY < edge.top + 32) scroller?.scrollBy(0, -12);
    else if (edge && event.clientY > edge.bottom - 32)
      scroller?.scrollBy(0, 12);

    const rows = [...(upNextList?.children ?? [])].map((row) =>
      row.getBoundingClientRect(),
    );

    drag.gap = gapAt(rows, event.clientY);
  }

  function drop(event: PointerEvent) {
    if (drag?.pointer !== event.pointerId) return;

    if (drag.gap !== null) moveTo(drag.from, drag.gap);

    drag = null;
  }

  /** Alt+↑ and Alt+↓ move the row; true when the key was handled. */
  function moveByKey(event: KeyboardEvent, from: number) {
    if (!event.altKey) return false;

    const gap =
      event.key === 'ArrowUp'
        ? from - 1
        : event.key === 'ArrowDown'
          ? from + 2
          : null;

    if (gap === null) return false;

    event.preventDefault();

    if (gap >= 0 && gap <= upNext.length && moveTo(from, gap)) {
      refocus = gap > from ? gap - 1 : gap;
    }

    return true;
  }

  function remove(index: number) {
    void attempt(() => api.queueRemove(index), onError);
  }

  function menu(event: MouseEvent, index: number) {
    openMenu(event, [
      {
        label: 'Play',
        icon: 'play',
        run: () => {
          play(index);
        },
      },
      {
        label: 'Play next',
        icon: 'list-start',
        disabled: index === upNext[0],
        run: () => {
          moveNext(index);
        },
      },
      'separator',
      {
        label: 'Remove from queue',
        icon: 'x',
        danger: true,
        run: () => {
          remove(index);
        },
      },
    ]);
  }
</script>

<aside class="queue" aria-labelledby="queue-title">
  <header>
    <h2 id="queue-title">Queue</h2>
    <button
      type="button"
      class="tertiary small"
      disabled={entries.length <= (playing ? 1 : 0)}
      title="Remove every track but the one playing"
      onclick={() => void attempt(api.clearQueue, onError)}>Clear</button
    >
    <button
      type="button"
      class="icon"
      aria-label="Close queue"
      title="Close"
      onclick={onClose}><Icon name="x" size={16} /></button
    >
  </header>

  <div class="scroll" bind:this={scroller}>
    {#if playing}
      <h3 class="group-title">Now playing</h3>
      <div class="now playing-mark">
        {#if playing.art}
          <img
            class="art"
            src={playing.art}
            alt=""
            loading="lazy"
            width="48"
            height="48"
          />
        {:else}
          <span class="art" aria-hidden="true"
            ><Icon name="disc-3" size={16} /></span
          >
        {/if}
        <span class="text">
          <strong class="truncate">{playing.title}</strong>
          {#if playing.artist}<small class="truncate">{playing.artist}</small
            >{/if}
        </span>
      </div>
    {/if}

    {#if entries.length === 0}
      <p class="muted empty">Nothing queued.</p>
    {:else if upNext.length === 0}
      <h3 class="group-title">Up next</h3>
      <p class="muted empty">Nothing after this track.</p>
    {:else}
      <h3 class="group-title">
        Up next <span class="num">{upNext.length}</span>
      </h3>
      <ol bind:this={upNextList} class:dragging={drag?.gap != null}>
        {#each upNext as index, order (index)}
          {@const entry = entries[index]}
          {@const row = rowEvents(
            () => (selected = index),
            () => {
              play(index);
            },
          )}
          {#if entry}
            <li
              data-drop={drag?.gap === order
                ? 'above'
                : drag?.gap === order + 1 && order === upNext.length - 1
                  ? 'below'
                  : undefined}
            >
              <button
                type="button"
                class="row entry"
                title={entry.title}
                aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                disabled={status.track === null && device === ''}
                data-selected={selected === index}
                data-dragged={drag?.gap != null && drag.from === order}
                onclick={row.onclick}
                ondblclick={row.ondblclick}
                onkeydown={(event) => {
                  if (!moveByKey(event, order)) row.onkeydown(event);
                }}
                onpointerdown={(event) => {
                  press(event, order);
                }}
                onpointermove={dragMove}
                onpointerup={drop}
                onpointercancel={() => (drag = null)}
                oncontextmenu={(event) => {
                  menu(event, index);
                }}
              >
                {#if entry.art}
                  <img
                    class="art"
                    src={entry.art}
                    alt=""
                    loading="lazy"
                    width="36"
                    height="36"
                  />
                {:else}
                  <span class="art" aria-hidden="true"
                    ><Icon name="disc-3" size={16} /></span
                  >
                {/if}
                <span class="text">
                  <span class="truncate">{entry.title}</span>
                  {#if entry.artist}<small class="truncate"
                      >{entry.artist}</small
                    >{/if}
                </span>
                <span class="time meta num"
                  >{entry.durationSeconds === null
                    ? ''
                    : clock(entry.durationSeconds)}</span
                >
              </button>
              <span class="actions">
                {#if order > 0}
                  <button
                    type="button"
                    class="icon"
                    aria-label={`Play ${entry.title} next`}
                    title="Play next"
                    onclick={() => {
                      moveNext(index);
                    }}><Icon name="list-start" size={16} /></button
                  >
                {/if}
                <button
                  type="button"
                  class="icon"
                  aria-label={`Remove ${entry.title} from the queue`}
                  title="Remove"
                  onclick={() => {
                    remove(index);
                  }}><Icon name="x" size={16} /></button
                >
              </span>
            </li>
          {/if}
        {/each}
      </ol>
    {/if}
  </div>
</aside>

<style>
  .queue {
    width: var(--queue-width);
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border-left: 1px solid var(--line);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3) var(--space-2) var(--space-5);
  }
  h2 {
    flex: 1;
    font-size: var(--type-title);
    line-height: var(--leading-title);
    font-weight: var(--weight-strong);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 var(--space-3) var(--space-5);
  }
  .group-title {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-4) var(--space-2) var(--space-2);
  }
  .group-title .num {
    color: var(--text-muted);
  }
  .now {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-s);
    position: relative;
    background: var(--surface-playing);
  }
  .now strong {
    font-weight: var(--weight-medium);
    color: var(--accent);
  }
  .now small {
    color: var(--text-on-emphasis);
  }
  .art {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    border-radius: var(--radius-s);
    object-fit: cover;
    background: var(--art-empty);
    color: var(--text-muted);
  }
  .now .art {
    width: 48px;
    height: 48px;
  }
  .entry .art {
    width: 36px;
    height: 36px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  small {
    color: var(--text-secondary);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
  }
  .empty {
    padding: var(--space-4) var(--space-2) 0;
    font-size: var(--type-small);
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    position: relative;
  }
  /* Where a dragged row will land: a line in the gap. */
  li[data-drop]::before {
    content: '';
    position: absolute;
    left: var(--space-2);
    right: var(--space-2);
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
    z-index: 1;
  }
  li[data-drop='above']::before {
    top: -1px;
  }
  li[data-drop='below']::before {
    bottom: -1px;
  }
  .entry[data-dragged='true'] {
    opacity: 0.5;
  }
  /* While dragging: no hover effects or row actions on the rows passed. */
  .dragging {
    cursor: grabbing;
  }
  .dragging li .actions {
    display: none;
  }
  .entry {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    text-align: left;
    font-weight: var(--weight-regular);
  }
  .time {
    font-size: var(--type-caption);
  }
  .entry[data-selected='true'] small {
    color: var(--text-on-emphasis);
  }
  /* Row actions replace the length on hover or keyboard focus. */
  .actions {
    position: absolute;
    top: 50%;
    right: var(--space-1);
    transform: translateY(-50%);
    display: flex;
    background: var(--surface-hover);
    border-radius: var(--radius-s);
    opacity: 0;
    pointer-events: none;
  }
  li:hover .actions,
  li:focus-within .actions {
    opacity: 1;
    pointer-events: auto;
  }
  li:where(:hover, :focus-within) {
    --row-idle-surface: var(--surface-hover);
  }
  li:has(.entry[data-selected='true']) .actions {
    background: var(--surface-selected-hover);
  }
  li:hover .time,
  li:focus-within .time {
    visibility: hidden;
  }
  .actions .icon {
    width: var(--hit-row);
    height: var(--hit-row);
    min-height: var(--hit-row);
  }
</style>

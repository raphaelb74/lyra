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

    if (forRevision === revision) entries = list ?? [];
  }

  $effect(() => {
    void load(revision);
  });

  /** Plays entry `index`; a stopped queue starts again from it. */
  function play(index: number) {
    void attempt(() => api.skipTo(index, device), onError);
  }

  function moveNext(index: number) {
    void attempt(() => api.queueMoveNext(index), onError);
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

  <div class="scroll">
    {#if playing}
      <h3 class="group-title">Now playing</h3>
      <div class="now">
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
      <p class="muted empty">
        Nothing queued. Play an album, or right-click music and choose Play next
        or Add to queue.
      </p>
    {:else if upNext.length === 0}
      <h3 class="group-title">Up next</h3>
      <p class="muted empty">Nothing after this track.</p>
    {:else}
      <h3 class="group-title">
        Up next <span class="num">{upNext.length}</span>
      </h3>
      <ol>
        {#each upNext as index, order (index)}
          {@const entry = entries[index]}
          {#if entry}
            <li>
              <button
                type="button"
                class="entry"
                title={entry.title}
                disabled={status.track === null && device === ''}
                onclick={() => {
                  play(index);
                }}
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
                <span class="time num"
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
  }
  .now strong {
    font-weight: var(--weight-medium);
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
    color: var(--text-muted);
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
  li:hover .entry,
  li:focus-within .entry {
    background: var(--surface-hover);
  }
  li:hover .time,
  li:focus-within .time {
    visibility: hidden;
  }
  .actions .icon {
    width: var(--hit-row);
    height: var(--hit-row);
  }
</style>

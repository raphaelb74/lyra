<script lang="ts">
  import { clock } from './format';
  import { attempt, type Backend, type QueueEntry } from './ipc';

  interface Props {
    api: Backend;
    /** Queue position playing now, or null. */
    current: number | null;
    /** With `current`, signals that the queue may have changed. */
    queueLength: number;
    onError: (message: string) => void;
  }

  const { api, current, queueLength, onError }: Props = $props();

  let entries = $state<QueueEntry[]>([]);

  /** Loads the queue; called again whenever its length or position changes. */
  async function load(length: number, position: number | null) {
    const list = await attempt(api.queue, onError);

    // Ignore a response for a queue that has changed since.
    if (length === queueLength && position === current) entries = list ?? [];
  }

  $effect(() => {
    void load(queueLength, current);
  });
</script>

<section aria-labelledby="queue-title">
  <div class="heading">
    <h3 id="queue-title">Queue</h3>
    <span class="muted">{entries.length}</span>
  </div>
  {#if entries.length === 0}
    <p class="muted">The queue is empty. Play an album to fill it.</p>
  {:else}
    <ol>
      {#each entries as entry, index (index)}
        <li>
          <button
            type="button"
            aria-current={index === current ? 'true' : undefined}
            onclick={() => {
              void attempt(() => api.skipTo(index), onError);
            }}
          >
            <span class="number">{index + 1}</span>
            <span class="title"
              >{entry.title}
              {#if entry.artist}<small>{entry.artist}</small>{/if}</span
            >
            <span class="time"
              >{entry.durationSeconds === null
                ? ''
                : clock(entry.durationSeconds)}</span
            >
          </button>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  .muted {
    color: var(--text-muted);
    font-size: 13px;
  }
  ol {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 2px;
  }
  button {
    width: 100%;
    display: grid;
    grid-template-columns: 3em minmax(0, 1fr) 4em;
    gap: 16px;
    align-items: center;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 10px 12px;
  }
  button:hover,
  button:focus-visible {
    background: var(--surface);
  }
  button[aria-current='true'] {
    background: var(--surface-current);
    color: var(--text-current);
  }
  .number,
  .time {
    color: var(--text-muted);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .time {
    text-align: right;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title small {
    color: var(--text-muted);
    margin-left: 8px;
  }
</style>

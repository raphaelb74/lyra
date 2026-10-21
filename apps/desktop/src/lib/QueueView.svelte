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
    <span class="muted num">{entries.length}</span>
  </div>
  {#if entries.length === 0}
    <p class="muted">The queue is empty. Play an album to fill it.</p>
  {:else}
    <ol class="rows">
      {#each entries as entry, index (index)}
        <li>
          <button
            type="button"
            class="row"
            aria-current={index === current ? 'true' : undefined}
            onclick={() => {
              void attempt(() => api.skipTo(index), onError);
            }}
          >
            <span class="meta end">{index + 1}</span>
            <span class="title"
              >{entry.title}
              {#if entry.artist}<small>{entry.artist}</small>{/if}</span
            >
            <span class="meta end"
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
  .rows {
    --columns: 3em minmax(0, 1fr) 4em;
  }
</style>

<script lang="ts">
  import Icon from './Icon.svelte';

  interface Props {
    onBack: () => void;
  }

  const { onBack }: Props = $props();

  // Written by scripts/record-notices.py; loaded only when this view opens.
  type Notices = typeof import('../assets/notices.json');

  let notices = $state<Notices | null>(null);
  let failed = $state(false);
  let search = $state('');
  /** The component whose licence text is shown. */
  let open = $state<string | null>(null);

  $effect(() => {
    import('../assets/notices.json').then(
      (module) => (notices = module.default),
      () => (failed = true),
    );
  });

  const shown = $derived.by(() => {
    const words = search.toLowerCase().split(/\s+/).filter(Boolean);
    const all = notices?.components ?? [];

    return all.filter((component) => {
      const text = `${component.name} ${component.license}`.toLowerCase();

      return words.every((word) => text.includes(word));
    });
  });
</script>

<button type="button" class="back" onclick={onBack}
  ><Icon name="chevron-left" size={16} />Settings</button
>
<section aria-labelledby="licences-title">
  <div class="heading">
    <h3 id="licences-title">Licences</h3>
  </div>
  <p class="muted intro">
    Lyra and its UNDA engine are MIT-licensed, and built on the open-source work
    below. Select one to read its licence.
  </p>
  {#if failed}
    <p class="muted">The licence list could not be loaded.</p>
  {:else if notices === null}
    <p class="muted">Loading…</p>
  {:else}
    {@const texts = notices.texts}
    <input
      type="search"
      placeholder="Search name or licence"
      aria-label="Search licences"
      bind:value={search}
    />
    <ul class="panel">
      {#each shown as component (`${component.name}@${component.version}`)}
        {@const id = `${component.name}@${component.version}`}
        <li>
          <button
            type="button"
            class="item"
            aria-expanded={open === id}
            onclick={() => (open = open === id ? null : id)}
          >
            <span class="truncate"
              >{component.name}
              <span class="muted num">{component.version}</span></span
            >
            <span class="muted truncate">{component.license}</span>
          </button>
          {#if open === id}
            {#each component.texts as index (index)}
              <pre>{texts[index]}</pre>
            {:else}
              <p class="muted none">
                No licence file is included with this package; it declares {component.license}.
              </p>
            {/each}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .intro {
    margin-bottom: var(--space-4);
    max-width: 60ch;
  }
  input {
    width: min(100%, 360px);
    margin-bottom: var(--space-3);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: var(--space-1);
  }
  .item {
    width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 14em);
    gap: var(--space-4);
    min-height: var(--hit-row);
    padding: var(--space-1) var(--space-3);
    border: 0;
    text-align: left;
    font-size: var(--type-small);
    font-weight: var(--weight-regular);
  }
  .item[aria-expanded='true'] {
    font-weight: var(--weight-medium);
  }
  pre,
  .none {
    margin: var(--space-1) var(--space-3) var(--space-3);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    color: var(--text-secondary);
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 360px;
    overflow-y: auto;
  }
</style>

<script lang="ts">
  import type { FilterGroup } from './filters';
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';

  interface Props {
    /** E.g. "Filter albums". */
    label: string;
    groups: readonly FilterGroup[];
    /** Something differs from the defaults: the button shows it. */
    active: boolean;
    onReset: () => void;
  }

  const { label, groups, active, onReset }: Props = $props();
</script>

<div class="filter" class:active>
  <Popover {label} triggerClass="trigger">
    {#snippet trigger()}
      <Icon name="list-filter" size={16} />
      <span>Filter</span>
    {/snippet}
    {#each groups as group (group.title)}
      <h2>{group.title}</h2>
      <ul>
        {#each group.options as option (option.value)}
          <li>
            <button
              type="button"
              class="choice"
              aria-current={option.value === group.value ? 'true' : undefined}
              onclick={() => {
                group.onChoose(option.value);
              }}
            >
              <span class="truncate">{option.label}</span>
              {#if option.value === group.value}
                <Icon name="check" size={16} />
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    {/each}
    <button
      type="button"
      class="tertiary small reset"
      disabled={!active}
      onclick={onReset}>Reset</button
    >
  </Popover>
</div>

<style>
  .filter :global(.trigger) {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    font-size: var(--type-small);
  }
  /* A filter is on: accent, as other toggles. */
  .active :global(.trigger) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .filter :global(.panel) {
    min-width: 220px;
    max-height: 420px;
  }
  h2 {
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    color: var(--text-muted);
    padding: var(--space-2) var(--space-2) var(--space-1);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .choice {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: var(--hit-row);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    text-align: left;
    font-size: var(--type-small);
    font-weight: var(--weight-regular);
  }
  .choice[aria-current='true'] {
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  .choice :global(svg) {
    color: var(--accent);
  }
  .reset {
    margin-top: var(--space-2);
    width: 100%;
  }
</style>

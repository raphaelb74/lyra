<script lang="ts" module>
  export interface Option<T> {
    value: T;
    label: string;
    hint: string;
  }
</script>

<script lang="ts" generics="T extends string | number">
  import type { Snippet } from 'svelte';

  interface Props {
    legend: string;
    /** Form name of the radio buttons; unique per group. */
    name: string;
    options: Option<T>[];
    value: T;
    disabled?: boolean;
    onChoose: (value: T) => void;
    /** Explanation shown under the legend. */
    children?: Snippet;
  }

  const {
    legend,
    name,
    options,
    value,
    disabled = false,
    onChoose,
    children,
  }: Props = $props();
</script>

<fieldset {disabled}>
  <legend>{legend}</legend>
  {#if children}
    <p class="muted">{@render children()}</p>
  {/if}
  {#each options as option (option.value)}
    <label>
      <input
        type="radio"
        {name}
        value={option.value}
        checked={value === option.value}
        onchange={() => {
          onChoose(option.value);
        }}
      />
      <span>{option.label}<small>{option.hint}</small></span>
    </label>
  {/each}
</fieldset>

<style>
  fieldset {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px 16px 16px;
    margin: 0 0 16px;
    max-width: 640px;
    display: grid;
    gap: 6px;
  }
  fieldset:disabled {
    opacity: 0.5;
  }
  legend {
    padding: 0 6px;
    font-weight: 500;
  }
  label {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 6px 4px;
    border-radius: 6px;
    cursor: pointer;
  }
  label:hover {
    background: var(--surface);
  }
  input {
    margin-top: 3px;
    accent-color: var(--accent-line);
  }
  label span {
    display: grid;
    gap: 2px;
  }
  small,
  .muted {
    color: var(--text-muted);
    font-size: 13px;
  }
  .muted {
    margin: 0 0 4px;
  }
</style>

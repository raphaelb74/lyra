<script lang="ts" generics="T extends string | number">
  import type { Option } from './segmented';

  interface Props {
    /** Id of the element naming the group. */
    labelledby: string;
    /** Form name of the radio buttons; unique per group. */
    name: string;
    options: Option<T>[];
    value: T;
    disabled?: boolean;
    onChoose: (value: T) => void;
  }

  const {
    labelledby,
    name,
    options,
    value,
    disabled = false,
    onChoose,
  }: Props = $props();
</script>

<!-- Radio buttons drawn as segments: arrow keys move the choice. -->
<div
  class="segmented"
  role="radiogroup"
  aria-labelledby={labelledby}
  aria-disabled={disabled}
>
  {#each options as option (option.value)}
    <label class:chosen={value === option.value}>
      <input
        type="radio"
        {name}
        value={option.value}
        checked={value === option.value}
        {disabled}
        onchange={() => {
          onChoose(option.value);
        }}
      />
      <span class="num">{option.label}</span>
    </label>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    flex-shrink: 0;
    padding: var(--space-0);
    gap: var(--space-0);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
  }
  .segmented[aria-disabled='true'] {
    opacity: 0.4;
  }
  label {
    position: relative;
    display: grid;
    place-items: center;
    min-height: var(--hit-row);
    padding: 0 var(--space-3);
    border-radius: var(--radius-s);
    font-size: var(--type-small);
    color: var(--text-secondary);
    white-space: nowrap;
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  label:hover {
    color: var(--text);
  }
  label.chosen {
    background: var(--surface-selected);
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  input {
    position: absolute;
    inset: 0;
    margin: 0;
    opacity: 0;
    cursor: inherit;
  }
  label:has(input:focus-visible) {
    outline: var(--focus-ring);
    outline-offset: 1px;
  }
  :global(:root[data-input='pointer']) label:has(input:focus-visible) {
    outline: none;
  }
  label:has(input:disabled) {
    cursor: default;
  }
</style>

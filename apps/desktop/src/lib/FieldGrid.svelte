<script lang="ts">
  import FieldInput from './FieldInput.svelte';
  import type { FieldInfo } from './ipc';

  interface Props {
    fields: FieldInfo[];
    /** Added to each field's accessible name, e.g. the track's file. */
    of?: string;
    valueOf: (info: FieldInfo) => string;
    isReset: (info: FieldInfo) => boolean;
    onInput: (info: FieldInfo, value: string) => void;
    onToggleReset: (info: FieldInfo) => void;
  }

  const { fields, of, valueOf, isReset, onInput, onToggleReset }: Props =
    $props();
</script>

<div class="fields">
  {#each fields as info (info.field)}
    <label class="field">
      <span class="label">{info.label}</span>
      <FieldInput
        {info}
        name={of === undefined ? info.label : `${info.label}, ${of}`}
        value={valueOf(info)}
        reset={isReset(info)}
        onInput={(value: string) => {
          onInput(info, value);
        }}
        onToggleReset={() => {
          onToggleReset(info);
        }}
      />
    </label>
  {/each}
</div>

<style>
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-3) var(--space-4);
  }
  .field {
    display: grid;
    gap: var(--space-1);
  }
  .label {
    color: var(--text-secondary);
    font-size: var(--type-caption);
  }
</style>

<script lang="ts">
  import Icon from './Icon.svelte';
  import type { FieldInfo } from './ipc';

  interface Props {
    info: FieldInfo;
    /** Accessible name. */
    name: string;
    /** What the field shows now, typed or saved. */
    value: string;
    /** The edit is to be dropped: the files' value will show. */
    reset: boolean;
    onInput: (value: string) => void;
    onToggleReset: () => void;
  }

  const { info, name, value, reset, onInput, onToggleReset }: Props = $props();
</script>

<span class="input">
  {#if info.field === 'compilation'}
    <input
      type="checkbox"
      aria-label={name}
      checked={!reset && value === '1'}
      disabled={reset}
      onchange={(event) => {
        onInput(event.currentTarget.checked ? '1' : '0');
      }}
    />
  {:else}
    <input
      type="text"
      aria-label={name}
      value={reset ? '' : value}
      placeholder={reset
        ? 'From the files'
        : info.multiple
          ? 'Separate values with ;'
          : ''}
      disabled={reset}
      oninput={(event) => {
        onInput(event.currentTarget.value);
      }}
    />
  {/if}
  {#if info.edited}
    <button
      type="button"
      class="icon"
      aria-pressed={reset}
      aria-label={`Use the files’ ${info.label.toLowerCase()}`}
      title={info.review
        ? 'Edited in Lyra; the file changed since. Use the files’ value'
        : 'Edited in Lyra. Use the files’ value'}
      onclick={onToggleReset}><Icon name="rotate-ccw" size={14} /></button
    >
  {/if}
</span>

<style>
  .input {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  input[type='text'] {
    flex: 1;
    min-width: 0;
  }
</style>

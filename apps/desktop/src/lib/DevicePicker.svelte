<script lang="ts">
  import Icon from './Icon.svelte';
  import type { DeviceInfo } from './ipc';
  import Popover from './Popover.svelte';

  interface Props {
    devices: DeviceInfo[];
    /** ALSA name of the chosen device; empty when none. */
    selected: string;
    /** The first listing has come back. */
    loaded: boolean;
    onSelect: (name: string) => void;
    onRefresh: () => Promise<void>;
  }

  const { devices, selected, loaded, onSelect, onRefresh }: Props = $props();

  let refreshing = $state(false);

  const current = $derived(devices.find((d) => d.name === selected));

  async function refresh() {
    refreshing = true;

    try {
      await onRefresh();
    } finally {
      refreshing = false;
    }
  }
</script>

<div class="picker" data-testid="devices" data-loaded={loaded}>
  <Popover
    label={current
      ? `Output device: ${current.label}`
      : 'Choose an output device'}
    triggerClass="trigger"
    above
  >
    {#snippet trigger()}
      <Icon name="speaker" size={16} />
      <span class="truncate label">{current?.label ?? 'No output'}</span>
      <Icon name="chevron-down" size={14} />
    {/snippet}
    {#snippet children(close: () => void)}
      <div class="head">
        <h2>Output</h2>
        <button
          type="button"
          class="icon"
          aria-label="Refresh devices"
          title="Refresh devices"
          disabled={refreshing}
          onclick={() => void refresh()}
          ><Icon name="refresh-cw" size={16} /></button
        >
      </div>
      {#if devices.length === 0}
        <p class="muted empty">
          {loaded
            ? 'No audio devices found. Connect your DAC, then refresh.'
            : 'Looking for devices…'}
        </p>
      {:else}
        <ul>
          {#each devices as device (device.name)}
            <li>
              <button
                type="button"
                class="choice"
                aria-current={device.name === selected ? 'true' : undefined}
                onclick={() => {
                  onSelect(device.name);
                  close();
                }}
              >
                <span class="truncate">{device.label}</span>
                {#if device.name === selected}
                  <Icon name="check" size={16} />
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/snippet}
  </Popover>
</div>

<style>
  /* The volume gives way; the name keeps its width (at most 220px). */
  .picker {
    flex: 0 0 auto;
    min-width: 0;
    display: flex;
    justify-content: flex-end;
  }
  .picker :global(.popover) {
    max-width: 100%;
  }
  .picker :global(.trigger) {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    max-width: 220px;
    min-width: 0;
    padding: var(--space-1) var(--space-2);
    border-color: transparent;
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  .picker :global(.trigger[aria-expanded='true']) {
    background: var(--surface-hover);
  }
  .label {
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 var(--space-1) var(--space-2);
  }
  h2 {
    font-size: var(--type-small);
    line-height: var(--leading-small);
    color: var(--text-secondary);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-0);
  }
  .choice {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: var(--hit);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    text-align: left;
    font-weight: var(--weight-regular);
  }
  .choice[aria-current='true'] {
    color: var(--accent);
  }
  .empty {
    padding: var(--space-2);
    max-width: 240px;
  }
</style>

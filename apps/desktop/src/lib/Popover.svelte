<script lang="ts">
  import { tick, type Snippet } from 'svelte';

  interface Props {
    /** Accessible name of the trigger and the panel. */
    label: string;
    /** Contents of the trigger button. */
    trigger: Snippet;
    triggerClass?: string;
    /** Opens upwards, for controls at the bottom of the window. */
    above?: boolean;
    /** Called as the panel opens, e.g. to load its contents; focus moves
     * into the panel once a returned promise settles. */
    onOpen?: () => void | Promise<void>;
    /** Panel contents; `close` returns focus to the trigger. */
    children: Snippet<[close: () => void]>;
  }

  const {
    label,
    trigger,
    triggerClass = '',
    above = false,
    onOpen,
    children,
  }: Props = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement | undefined>();
  let button = $state<HTMLButtonElement | undefined>();
  let panel = $state<HTMLDivElement | undefined>();

  function focusable(): HTMLElement[] {
    return Array.from(
      panel?.querySelectorAll<HTMLElement>('button:not(:disabled), input') ??
        [],
    );
  }

  async function show() {
    open = true;
    await onOpen?.();
    await tick();

    const items = focusable();
    const current = items.find((item) => item.ariaCurrent === 'true');

    // Nothing to focus inside: the panel itself, so Escape still closes it.
    (current ?? items[0] ?? panel)?.focus();
  }

  function close() {
    open = false;
    button?.focus();
  }

  /** Escape closes; arrow keys move between the panel's controls. */
  function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      close();

      return;
    }

    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;

    const items = focusable();

    if (items.length === 0 || event.target instanceof HTMLInputElement) return;

    const index = items.findIndex((item) => item === document.activeElement);
    const step = event.key === 'ArrowDown' ? 1 : -1;
    const next = (index + step + items.length) % items.length;

    event.preventDefault();
    items[next]?.focus();
  }

  /** A press outside closes the panel without taking focus back. */
  function onPointer(event: PointerEvent) {
    const target = event.target;

    if (open && !(target instanceof Node && root?.contains(target))) {
      open = false;
    }
  }
</script>

<svelte:document onpointerdown={onPointer} />

<div class="popover" bind:this={root}>
  <button
    type="button"
    class={triggerClass}
    aria-label={label}
    aria-expanded={open}
    aria-haspopup="dialog"
    title={label}
    bind:this={button}
    onclick={() => {
      if (open) close();
      else void show();
    }}>{@render trigger()}</button
  >
  {#if open}
    <div
      class="panel"
      class:above
      role="dialog"
      aria-label={label}
      tabindex="-1"
      bind:this={panel}
      onkeydown={onKey}
    >
      {@render children(close)}
    </div>
  {/if}
</div>

<style>
  .popover {
    position: relative;
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .panel {
    position: absolute;
    right: 0;
    top: calc(100% + var(--space-1));
    z-index: 10;
    min-width: 240px;
    max-height: 360px;
    overflow-y: auto;
    padding: var(--space-2);
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-pop);
    animation: appear var(--dur-panel) var(--ease);
  }
  .panel.above {
    top: auto;
    bottom: calc(100% + var(--space-2));
  }
  .panel:focus {
    outline: none;
  }
  @keyframes appear {
    from {
      opacity: 0;
      transform: translateY(var(--space-1));
    }
  }
</style>

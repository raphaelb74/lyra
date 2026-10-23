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
    /** The panel's edge lined up with the trigger's: its right edge (the
     * default), or centred on it. */
    align?: 'end' | 'center';
    /** With `above`: an ancestor (CSS selector) the panel opens clear of,
     * e.g. the player bar, so it never covers its other controls. */
    clear?: string;
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
    align = 'end',
    clear,
    onOpen,
    children,
  }: Props = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement | undefined>();
  let button = $state<HTMLButtonElement | undefined>();
  let panel = $state<HTMLDivElement | undefined>();
  /** How far the panel rises past the trigger to clear `clear`, in px. */
  let lift = $state(0);

  function focusable(): HTMLElement[] {
    return Array.from(
      panel?.querySelectorAll<HTMLElement>('button:not(:disabled), input') ??
        [],
    );
  }

  async function show() {
    const edge = clear === undefined ? null : root?.closest(clear);

    lift =
      above && edge && root
        ? Math.max(
            0,
            root.getBoundingClientRect().top - edge.getBoundingClientRect().top,
          )
        : 0;
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

<div class="popover" style:--lift={`${String(lift)}px`} bind:this={root}>
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
      class:center={align === 'center'}
      role="dialog"
      aria-label={label}
      tabindex="-1"
      bind:this={panel}
      onkeydown={onKey}
    >
      {@render children(close)}
    </div>
    <span class="arrow" class:above aria-hidden="true"></span>
  {/if}
</div>

<style>
  /* The panel sits a little away from its trigger, with an arrow on the
     trigger's centre line that covers the panel's border. */
  .popover {
    --offset: var(--space-3);
    --arrow: 10px;

    position: relative;
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .panel {
    position: absolute;
    right: 0;
    top: calc(100% + var(--offset));
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
  .panel.center {
    right: auto;
    left: 50%;
    translate: -50% 0;
  }
  .panel.above {
    top: auto;
    bottom: calc(100% + var(--offset) + var(--lift));
  }
  /* A square turned 45° and centred on the panel's edge. Only its outer
     half shows (plus 2 px, to cover the panel's border there), so nothing
     overlaps the panel and the tip stays crisp. */
  .arrow {
    position: absolute;
    z-index: 11;
    left: calc(50% - var(--arrow) / 2);
    top: calc(100% + var(--offset) - var(--arrow) / 2);
    width: var(--arrow);
    height: var(--arrow);
    background: var(--surface-raised);
    border-top: 1px solid var(--line);
    border-left: 1px solid var(--line);
    border-top-left-radius: 2px;
    clip-path: polygon(0 0, 100% 0, 100% 2px, 2px 100%, 0 100%);
    rotate: 45deg;
    pointer-events: none;
    animation: appear var(--dur-panel) var(--ease);
  }
  .arrow.above {
    top: auto;
    bottom: calc(100% + var(--offset) + var(--lift) - var(--arrow) / 2);
    rotate: 225deg;
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

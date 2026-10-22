<script lang="ts">
  import { tick } from 'svelte';

  import Icon from './Icon.svelte';
  import { closeMenu, menu, type MenuItem } from './menu.svelte';

  let panel = $state<HTMLDivElement | undefined>();
  /** Where the panel sits once it is known to fit in the window. */
  let place = $state({ x: 0, y: 0 });

  function buttons(): HTMLButtonElement[] {
    return Array.from(
      panel?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [],
    );
  }

  function choose(item: Extract<MenuItem, { run: () => void }>) {
    item.run();

    if (!item.keep) closeMenu();
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeMenu();

      return;
    }

    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;

    if (event.target instanceof HTMLInputElement) return;

    const items = buttons();
    const index = items.findIndex((item) => item === document.activeElement);
    const step = event.key === 'ArrowDown' ? 1 : -1;

    event.preventDefault();
    items[(index + step + items.length) % items.length]?.focus();
  }

  function onPointer(event: PointerEvent) {
    const target = event.target;

    if (
      menu.open !== null &&
      !(target instanceof Node && panel?.contains(target))
    ) {
      closeMenu();
    }
  }

  // Keep the panel inside the window, and focus its first item.
  $effect(() => {
    const open = menu.open;

    if (open === null) return;

    place = { x: open.x, y: open.y };
    void tick().then(() => {
      if (panel === undefined) return;

      const box = panel.getBoundingClientRect();

      place = {
        x: Math.min(open.x, window.innerWidth - box.width - 8),
        y:
          open.y + box.height > window.innerHeight - 8
            ? Math.max(8, open.y - box.height)
            : open.y,
      };
      (buttons()[0] ?? panel.querySelector('input'))?.focus();
    });
  });
</script>

<svelte:document onpointerdown={onPointer} />
<svelte:window onblur={closeMenu} onresize={closeMenu} />

{#if menu.open}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={panel}
    style:left={`${String(place.x)}px`}
    style:top={`${String(place.y)}px`}
    onkeydown={onKey}
    oncontextmenu={(event) => {
      event.preventDefault();
    }}
  >
    {#each menu.open.items as item, index (index)}
      {#if item === 'separator'}
        <hr />
      {:else if 'field' in item}
        <form
          onsubmit={(event) => {
            event.preventDefault();

            const field = event.currentTarget.elements.namedItem('name');

            const text =
              field instanceof HTMLInputElement ? field.value.trim() : '';

            if (text === '') return;

            item.submit(text);
            closeMenu();
          }}
        >
          <input
            name="name"
            aria-label={item.field}
            placeholder={item.field}
            maxlength="200"
          />
        </form>
      {:else}
        <button
          type="button"
          role="menuitem"
          class:danger={item.danger}
          disabled={item.disabled}
          onclick={() => {
            choose(item);
          }}
        >
          <span class="icon-slot"
            >{#if item.icon}<Icon name={item.icon} size={16} />{/if}</span
          >
          {item.label}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 220px;
    max-width: 320px;
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--space-1);
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-pop);
    animation: appear var(--dur-fast) var(--ease);
  }
  .menu:focus {
    outline: none;
  }
  button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--hit);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    text-align: left;
    font-size: var(--type-small);
    font-weight: var(--weight-regular);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button:focus-visible {
    outline: none;
    background: var(--surface-hover);
  }
  button.danger {
    border: 0;
  }
  .icon-slot {
    display: inline-grid;
    width: 16px;
    color: var(--text-secondary);
  }
  hr {
    margin: var(--space-1) var(--space-2);
    border: 0;
    border-top: 1px solid var(--line);
  }
  form {
    padding: var(--space-1);
  }
  input {
    width: 100%;
  }
  @keyframes appear {
    from {
      opacity: 0;
    }
  }
</style>

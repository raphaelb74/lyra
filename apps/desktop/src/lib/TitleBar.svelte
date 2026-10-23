<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';

  import Icon from './Icon.svelte';
  import LyraMark from './LyraMark.svelte';

  /**
   * Lyra draws its own title bar (the system one is off in tauri.conf.json).
   * Both halves drag the window and a double-click maximizes it; the left
   * half continues the sidebar, the right half the content.
   */

  const native = isTauri();

  let maximized = $state(false);

  async function follow() {
    maximized = await getCurrentWindow().isMaximized();
  }

  onMount(() => {
    if (!native) return;

    void follow();

    const unlisten = getCurrentWindow().onResized(() => {
      void follow();
    });

    return () => {
      void unlisten.then((stop) => {
        stop();
      });
    };
  });
</script>

<header class="titlebar">
  <div class="side" data-tauri-drag-region>
    <span class="wordmark" data-tauri-drag-region
      ><span class="mark" data-tauri-drag-region><LyraMark size={26} /></span
      ><span class="name" data-tauri-drag-region>Lyra</span></span
    >
  </div>
  <div class="main" data-tauri-drag-region>
    {#if native}
      <div class="controls">
        <button
          type="button"
          class="icon"
          aria-label="Minimize"
          title="Minimize"
          onclick={() => void getCurrentWindow().minimize()}
          ><Icon name="minus" size={16} /></button
        >
        <button
          type="button"
          class="icon"
          aria-label={maximized ? 'Restore' : 'Maximize'}
          title={maximized ? 'Restore' : 'Maximize'}
          onclick={() => void getCurrentWindow().toggleMaximize()}
          ><Icon name={maximized ? 'copy' : 'square'} size={14} /></button
        >
        <button
          type="button"
          class="icon close"
          aria-label="Close"
          title="Close"
          onclick={() => void getCurrentWindow().close()}
          ><Icon name="x" size={16} /></button
        >
      </div>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    display: flex;
    height: var(--titlebar-height);
    user-select: none;
  }
  /* The wordmark is centred over the sidebar. */
  .side {
    flex: 0 0 var(--sidebar-width);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 var(--space-4);
    background: var(--surface);
    border-right: 1px solid var(--line);
  }
  .wordmark {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-family: var(--font-brand);
    font-size: var(--type-brand);
    line-height: 1;
    font-weight: var(--weight-strong);
    letter-spacing: 0.3em;
    text-transform: uppercase;
    color: var(--text);
  }
  .mark {
    display: flex;
    color: var(--accent);
  }
  /* Letter spacing trails the last letter; take it back to stay centred. */
  .name {
    margin-right: -0.3em;
  }
  .main {
    flex: 1;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    padding: 0 var(--space-2);
  }
  .controls {
    display: flex;
    gap: var(--space-1);
  }
  .controls button {
    width: var(--hit);
    height: var(--hit-row);
    color: var(--text-muted);
  }
  .controls button:hover:not(:disabled) {
    color: var(--text);
  }
  .controls .close:hover:not(:disabled) {
    background: var(--danger-surface);
    color: var(--danger);
  }
  @media (max-width: 1000px) {
    .side {
      justify-content: center;
      padding: 0;
    }
    .name {
      display: none;
    }
  }
</style>

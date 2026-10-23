<script lang="ts">
  import Icon from './Icon.svelte';
  import { dismissToast, holdToast, resumeToast, toast } from './toast.svelte';

  function act() {
    const action = toast.current?.action;

    dismissToast();
    action?.run();
  }
</script>

{#if toast.current}
  {@const { text, detail, action } = toast.current}
  <div
    class="toast"
    role="status"
    onmouseenter={holdToast}
    onmouseleave={resumeToast}
    onfocusin={holdToast}
    onfocusout={resumeToast}
  >
    <span class="mark"><Icon name="check" size={14} /></span>
    <span class="text">
      {text}{#if detail}<span class="detail">{detail}</span>{/if}
    </span>
    {#if action}
      <button type="button" class="tertiary small action" onclick={act}
        >{action.label}</button
      >
    {/if}
    <button
      type="button"
      class="icon close"
      aria-label="Dismiss"
      onclick={dismissToast}><Icon name="x" size={14} /></button
    >
  </div>
{/if}

<style>
  /* Centred at the foot of the view, just above the player bar. */
  .toast {
    position: absolute;
    bottom: var(--space-4);
    left: 50%;
    translate: -50% 0;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    max-width: calc(100% - 2 * var(--space-5));
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
    background: var(--surface-raised);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-pop);
    font-size: var(--type-small);
    animation: rise var(--dur-panel) var(--ease);
  }
  .mark {
    display: inline-flex;
    color: var(--accent);
  }
  .text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-right: var(--space-1);
  }
  .detail {
    color: var(--text-muted);
  }
  .detail::before {
    content: '·';
    margin-inline: var(--space-2);
  }
  .action {
    color: var(--accent);
    font-weight: 600;
  }
  .close {
    --hit: var(--control-height-small);

    color: var(--text-muted);
  }
  @keyframes rise {
    from {
      opacity: 0;
      translate: -50% var(--space-2);
    }
  }
</style>

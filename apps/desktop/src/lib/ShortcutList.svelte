<script lang="ts">
  import { keyGroups, SHORTCUTS } from './shortcuts';
</script>

<dl class="panel keys">
  {#each SHORTCUTS as { label, presses } (label)}
    <div class="shortcut">
      <dt>{label}</dt>
      <dd>
        {#each keyGroups(presses) as group, index (index)}
          {#if index > 0}<span class="or">or</span>{/if}
          {#each group.modifiers as modifier (modifier)}
            <kbd>{modifier}</kbd><span class="plus">+</span>
          {/each}
          {#each group.keys as key, alternative (key)}
            {#if alternative > 0}<span class="or">or</span>{/if}<kbd>{key}</kbd>
          {/each}
        {/each}
      </dd>
    </div>
  {/each}
</dl>

<style>
  /* Two columns when there is room, filled top to bottom: playback on the
     left, navigation on the right. */
  .keys {
    columns: 2 300px;
    column-gap: var(--space-6);
    padding-block: var(--space-2);
  }
  .shortcut {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-height: var(--row-height);
    break-inside: avoid;
  }
  dd {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    white-space: nowrap;
  }
  kbd {
    font-family: inherit;
    display: inline-block;
    min-width: 24px;
    padding: var(--space-0) var(--space-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-s);
    font-size: var(--type-small);
    line-height: var(--leading-small);
    text-align: center;
  }
  .or,
  .plus {
    color: var(--text-secondary);
    font-size: var(--type-small);
  }
</style>

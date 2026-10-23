<script lang="ts">
  import Icon from './Icon.svelte';
  import { openMenu, type MenuItem } from './menu.svelte';

  interface Props {
    /** Accessible name, e.g. "More for Aladdin Sane". */
    label: string;
    /** The row's right-click menu, built when it opens. */
    items: () => MenuItem[];
  }

  const { label, items }: Props = $props();
</script>

<!-- The row's right-click menu, for the pointer and keyboard alike. -->
<button
  type="button"
  class="icon"
  aria-label={label}
  aria-haspopup="menu"
  title="More"
  onclick={(event) => {
    openMenu(event, items());
  }}><Icon name="ellipsis" size={16} /></button
>

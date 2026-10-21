<script lang="ts">
  import { icons, type IconName } from '../assets/icons/lucide';

  interface Props {
    name: IconName;
    /** Pixels; the stroke stays 1.5 px at any size. */
    size?: number;
  }

  const { name, size = 20 }: Props = $props();
</script>

<!-- Decorative: the control around it carries the accessible name. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  aria-hidden="true"
  focusable="false"
>
  {#each icons[name] as [tag, attributes], index (index)}
    <svelte:element
      this={tag}
      xmlns="http://www.w3.org/2000/svg"
      {...attributes}
    />
  {/each}
</svg>

<style>
  svg {
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  svg :global(*) {
    vector-effect: non-scaling-stroke;
  }
</style>

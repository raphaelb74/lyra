<script lang="ts">
  // "LYRA" drawn, not typeset: thin geometric capitals after Raleway Light,
  // with the brand's crossbar-less Λ, which no font has. Cap height is 100
  // units, so `height` is roughly the cap height in pixels.
  const { height = 14 }: { height?: number } = $props();
</script>

<svg
  width={(height * 472) / 103}
  {height}
  viewBox="-1 -1 472 103"
  role="img"
  aria-label="Lyra"
  data-tauri-drag-region
>
  <path
    d="M3 0V97H68M108 0L156 51L204 0M156 51V100M247 100V3H293A24.5 24.5 0 0 1 293 52H247M282 52L326 100M367 100L416.5 7L466 100"
  />
</svg>

<style>
  path {
    fill: none;
    stroke: currentColor;
    stroke-width: 6;
    stroke-miterlimit: 12;
  }
</style>

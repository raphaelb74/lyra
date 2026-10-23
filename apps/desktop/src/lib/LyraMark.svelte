<script lang="ts">
  // The Lyra lyre (brand guide): decorative, drawn in the active accent.
  // assets/lyra.svg holds the same paths for the app icon: change both.
  const { size = 20 }: { size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  aria-hidden="true"
  focusable="false"
  data-tauri-drag-region
>
  <path
    class="lyre"
    d="M5.2 6.5C4.9 7.2 3.5 7.2 3.1 6.2 2.7 5.1 3.5 4 4.7 4 6.1 4 6.7 6.4 6.7 9.2v1.4c0 3.3 2.3 6 5.3 6s5.3-2.7 5.3-6V9.2c0-2.8.6-5.2 2-5.2 1.2 0 2 1.1 1.6 2.2-.4 1-1.8 1-2.1.3M12 16.6v3.8M9.2 20.4h5.6M9.7 7.8l.6 8.2M12 7.4v9.2M14.3 7.8l-.6 8.2"
  />
  <path
    d="M12 1.2q.35 1.65 2 2-1.65.35-2 2-.35-1.65-2-2 1.65-.35 2-2z"
    fill="currentColor"
  />
</svg>

<style>
  .lyre {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>

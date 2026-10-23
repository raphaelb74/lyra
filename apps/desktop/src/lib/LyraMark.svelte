<script lang="ts">
  // The Lyra lyre (brand guide): decorative, drawn in the active accent.
  // The same paths: assets/lyra.svg (app icon), docs/assets/lyra-mark.svg
  // (README). Change all three together.
  const { size = 20 }: { size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="62 47 320 320"
  aria-hidden="true"
  focusable="false"
  data-tauri-drag-region
>
  <g class="lyre">
    <path
      stroke-width="17"
      d="M220 300 C168 281 153 242 153 180 L153 138 C153 117 140 106 124 110 C103 115 106 144 127 144 C140 144 149 132 149 124 M220 300 C272 281 287 242 287 180 L287 138 C287 117 300 106 316 110 C337 115 334 144 313 144 C300 144 291 132 291 124 M220 302 V344 M187 352 H253"
    />
    <path stroke-width="15" d="M187 165 V262 M220 159 V276 M253 165 V262" />
  </g>
  <path
    fill="currentColor"
    d="M220 53 C226 73 233 80 253 86 C233 92 226 99 220 119 C214 99 207 92 187 86 C207 80 214 73 220 53Z"
  />
</svg>

<style>
  .lyre {
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>

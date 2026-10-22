<script lang="ts">
  import dsd from '../assets/formats/dsd.svg';
  import flac from '../assets/formats/flac.svg';

  interface Props {
    /** Which logo; see assets/formats/NOTICE.md. */
    format: 'dsd' | 'flac';
  }

  const { format }: Props = $props();

  const LOGOS = {
    dsd: { url: dsd, name: 'DSD', ratio: 500 / 253.77 },
    flac: { url: flac, name: 'FLAC', ratio: 262 / 130 },
  } as const;

  const logo = $derived(LOGOS[format]);
</script>

<!-- Drawn as a mask in the text colour, so it follows the theme. -->
<span
  class="logo"
  role="img"
  aria-label={logo.name}
  title={logo.name}
  style:--logo={`url("${logo.url}")`}
  style:--ratio={logo.ratio}
></span>

<style>
  .logo {
    display: inline-block;
    height: 24px;
    aspect-ratio: var(--ratio);
    background: currentColor;
    -webkit-mask: var(--logo) center / contain no-repeat;
    mask: var(--logo) center / contain no-repeat;
  }
</style>

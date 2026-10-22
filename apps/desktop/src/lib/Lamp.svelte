<script lang="ts">
  import { formatTone } from './format';
  import type { Track } from './ipc';

  const { track }: { track: Track } = $props();
</script>

<!-- Colour: what the DAC receives. Lit: bit-perfect, verified by the engine;
     a ring otherwise. -->
<span
  class="lamp"
  data-tone={formatTone(track) ?? 'none'}
  data-status={track.bitPerfect?.status ?? 'none'}
  aria-hidden="true"
></span>

<style>
  .lamp {
    --tone: var(--text-muted);
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: var(--radius-round);
    border: 1px solid var(--tone);
  }
  .lamp[data-tone='cd'] {
    --tone: var(--tone-cd);
  }
  .lamp[data-tone='hiRes'] {
    --tone: var(--tone-hi-res);
  }
  .lamp[data-tone='dsd'] {
    --tone: var(--tone-dsd);
  }
  .lamp[data-tone='dsdHigh'] {
    --tone: var(--tone-dsd-high);
  }
  .lamp[data-status='yes'] {
    background: var(--tone);
    box-shadow: 0 0 4px var(--tone);
  }
</style>

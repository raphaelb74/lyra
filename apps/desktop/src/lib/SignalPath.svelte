<script lang="ts">
  import { verdictLabel } from './format';
  import type { DeviceInfo, Track } from './ipc';
  import { levelling } from './levelling.svelte';
  import { signalStages } from './signal';

  interface Props {
    track: Track;
    /** The playing device, for its name; undefined when it is gone. */
    device: DeviceInfo | undefined;
  }

  const { track, device }: Props = $props();

  // ALSA names and the kernel's hw_params stay in Diagnostics.
  const stages = $derived(
    signalStages(
      track,
      device?.label ?? track.device,
      levelling[track.device] ?? null,
    ),
  );
  const output = $derived(track.output);
</script>

<ol class="path">
  {#each stages as stage (stage.title)}
    <li>
      <span class="title">{stage.title}</span>
      <span class="text">{stage.text}</span>
    </li>
  {/each}
</ol>
<p class="verdict" data-status={track.bitPerfect?.status ?? 'none'}>
  {output === null ? 'Unverified' : verdictLabel(track.bitPerfect)}
  {#if output !== null && track.bitPerfect?.reason}
    <span class="reason">{track.bitPerfect.reason}</span>
  {/if}
</p>

<style>
  .path {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  /* Stages joined by a hairline, a dot at each. */
  li {
    position: relative;
    display: grid;
    padding: 0 0 var(--space-3) var(--space-5);
    font-size: var(--type-small);
    line-height: var(--leading-small);
  }
  li::before {
    content: '';
    position: absolute;
    left: 3px;
    top: 0.45em;
    width: 7px;
    height: 7px;
    border-radius: var(--radius-round);
    border: 1px solid var(--line-strong);
    background: var(--surface);
  }
  li:not(:last-child)::after {
    content: '';
    position: absolute;
    left: 6px;
    top: calc(0.45em + 8px);
    bottom: 0;
    width: 1px;
    background: var(--line-strong);
  }
  .title {
    color: var(--text-muted);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    font-weight: var(--weight-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .text {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .reason {
    display: block;
    color: var(--text-muted);
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    font-weight: var(--weight-regular);
  }
  .verdict {
    margin: var(--space-1) 0 0;
    padding-top: var(--space-3);
    border-top: 1px solid var(--line);
    font-size: var(--type-small);
    font-weight: var(--weight-medium);
    color: var(--text-secondary);
  }
  .verdict[data-status='yes'] {
    color: var(--accent);
  }
  .verdict[data-status='no'] {
    color: var(--warning);
  }
</style>

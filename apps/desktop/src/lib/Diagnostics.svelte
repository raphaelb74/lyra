<script lang="ts">
  import {
    conversionLabel,
    diagnosticsReport,
    framesLabel,
    supportLabel,
  } from './diagnostics';
  import { outputLabel } from './format';
  import Icon from './Icon.svelte';
  import {
    attempt,
    type Backend,
    type Capabilities,
    type DeviceInfo,
    type DiagnosticsInfo,
    type PlayerStatus,
  } from './ipc';

  interface Props {
    api: Backend;
    /** Selected output device; undefined when none. */
    device: DeviceInfo | undefined;
    status: PlayerStatus;
    onBack: () => void;
    onError: (message: string) => void;
  }

  const { api, device, status, onBack, onError }: Props = $props();

  /** How often the live figures are read while this view is open. */
  const REFRESH_MS = 1000;

  let info = $state<DiagnosticsInfo | null>(null);
  let capabilities = $state<Capabilities | null>(null);
  let probeMessage = $state<string | null>(null);
  let probing = $state(false);
  let copyMessage = $state<string | null>(null);
  /** Shown for manual copying when the clipboard refuses. */
  let fallbackText = $state<string | null>(null);

  const output = $derived(
    status.track !== null && status.track.device === info?.device
      ? status.track.output
      : null,
  );
  const playback = $derived(info?.playback ?? null);
  const rate = $derived(output?.sampleRate ?? null);

  // Live figures for the selected device, refreshed while the view is open.
  $effect(() => {
    const name = device?.name;

    info = null;
    capabilities = null;
    probeMessage = null;
    fallbackText = null;
    copyMessage = null;

    if (name === undefined) return;

    let current = true;

    const read = () => {
      void attempt(() => api.diagnostics(name), onError).then((next) => {
        if (current && next !== undefined) info = next;
      });
    };

    read();
    const timer = setInterval(read, REFRESH_MS);

    return () => {
      current = false;
      clearInterval(timer);
    };
  });

  async function probe(name: string) {
    probing = true;
    probeMessage = null;

    try {
      capabilities = await api.probeDevice(name);
    } catch (error) {
      capabilities = null;
      probeMessage = error instanceof Error ? error.message : String(error);
    } finally {
      probing = false;
    }
  }

  async function copy(current: DiagnosticsInfo) {
    const text = diagnosticsReport(device, current, output, capabilities);

    fallbackText = null;

    try {
      await navigator.clipboard.writeText(text);
      copyMessage = 'Report copied.';
    } catch {
      copyMessage = 'Copying is blocked here. Select the text below instead.';
      fallbackText = text;
    }
  }
</script>

<button type="button" class="back" onclick={onBack}
  ><Icon name="chevron-left" size={16} />Settings</button
>
<section aria-labelledby="diagnostics-title">
  <div class="heading">
    <h3 id="diagnostics-title">Diagnostics</h3>
    {#if info}
      {@const current = info}
      <button type="button" class="small" onclick={() => void copy(current)}
        >Copy report</button
      >
    {/if}
    {#if copyMessage}
      <span class="note" role="status">{copyMessage}</span>
    {/if}
  </div>

  {#if device === undefined}
    <p class="muted">Choose an output device to see its details.</p>
  {:else if info === null}
    <p class="muted">Reading {device.name}…</p>
  {:else}
    <p class="device">
      {device.label}
      <span class="muted">{info.device}</span>
    </p>

    {#if fallbackText !== null}
      <textarea readonly rows="12" aria-label="Diagnostics report"
        >{fallbackText}</textarea
      >
    {/if}

    <h4>Playback</h4>
    {#if playback === null}
      <p class="muted">Lyra is not playing on this device.</p>
    {:else}
      <dl>
        <dt>Output</dt>
        <dd>{output === null ? 'Opening…' : outputLabel(output)}</dd>
        <dt>Underruns</dt>
        <dd data-warn={playback.underruns > 0}>
          {playback.underruns === 0
            ? 'None since the device opened'
            : `${String(playback.underruns)} since the device opened`}
        </dd>
        <dt>Buffer</dt>
        <dd>
          {framesLabel(playback.bufferFrames, rate)}, period
          {framesLabel(playback.periodFrames, rate)}
        </dd>
        {#if playback.takenFrom !== null}
          <dt>Handed over by</dt>
          <dd>{playback.takenFrom}</dd>
        {/if}
        {#if playback.conversion !== null}
          <dt>Conversion</dt>
          <dd data-warn={playback.conversion.clipped > 0}>
            {conversionLabel(playback.conversion)}
          </dd>
        {/if}
      </dl>
    {/if}

    <h4>Kernel</h4>
    <dl>
      <dt>Kernel</dt>
      <dd>{info.kernel ?? 'Unknown'}</dd>
      <dt>ALSA</dt>
      <dd>{info.alsa ?? 'Unknown'}</dd>
      {#if info.usbId !== null}
        <dt>USB ID</dt>
        <dd>{info.usbId}</dd>
      {/if}
    </dl>
    {#if info.hwParams === null}
      <p class="muted">The device is closed: no program is playing on it.</p>
    {:else}
      <p class="caption">What the hardware runs at now</p>
      <pre>{info.hwParams}</pre>
    {/if}
    {#if info.usbStream !== null}
      <p class="caption">USB stream, with the rate the DAC reports back</p>
      <pre>{info.usbStream}</pre>
    {/if}

    <h4>What the device accepts</h4>
    {#if capabilities}
      <p class="muted">
        {capabilities.channelsMin === capabilities.channelsMax
          ? `${String(capabilities.channelsMin)} channels`
          : `${String(capabilities.channelsMin)} to ${String(capabilities.channelsMax)} channels`}
      </p>
      <ul class="formats">
        {#each capabilities.formats as support (support.format)}
          <li>{supportLabel(support)}</li>
        {/each}
      </ul>
    {:else}
      <p class="muted">
        Lyra opens the device for a moment and asks the driver. This fails while
        another program plays on it.
      </p>
    {/if}
    <button
      type="button"
      class="small"
      disabled={probing}
      onclick={() => void probe(device.name)}
      >{probing ? 'Asking the driver…' : 'Ask the driver'}</button
    >
    {#if probeMessage}
      <p class="note" role="alert">{probeMessage}</p>
    {/if}
  {/if}
</section>

<style>
  .heading {
    justify-content: flex-start;
  }
  h4 {
    font-size: var(--type-body);
    margin: var(--space-6) 0 var(--space-3);
  }
  .device {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
  }
  .caption {
    color: var(--text-secondary);
    font-size: var(--type-small);
    margin: var(--space-3) 0 var(--space-2);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-5);
    margin: 0;
    font-size: var(--type-small);
    font-variant-numeric: tabular-nums;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
  }
  dd[data-warn='true'] {
    color: var(--warning);
  }
  pre,
  textarea {
    margin: 0;
    padding: var(--space-3);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-s);
    font-family: var(--font-mono);
    font-size: var(--type-caption);
    line-height: var(--leading-small);
    overflow-x: auto;
    color: var(--text);
  }
  textarea {
    width: 100%;
    margin: var(--space-3) 0 0;
    resize: vertical;
  }
  .formats {
    margin: 0 0 var(--space-3);
    padding-left: var(--space-5);
    font-size: var(--type-small);
    font-variant-numeric: tabular-nums;
    display: grid;
    gap: var(--space-1);
  }
</style>

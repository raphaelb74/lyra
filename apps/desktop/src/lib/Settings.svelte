<script lang="ts">
  import {
    attempt,
    type AppInfo,
    type Backend,
    type DeviceInfo,
    type Settings,
  } from './ipc';
  import LibraryFolders from './LibraryFolders.svelte';
  import Segmented from './Segmented.svelte';
  import type { Option } from './segmented';

  interface Props {
    api: Backend;
    /** Name and version; null until the audio engine answers. */
    appInfo: AppInfo | null;
    connected: boolean;
    checking: boolean;
    /** Selected output device, for its diagnostics. */
    device: DeviceInfo | undefined;
    onCheck: () => void;
    onOpenDiagnostics: () => void;
    onError: (message: string) => void;
  }

  const {
    api,
    appInfo,
    connected,
    checking,
    device,
    onCheck,
    onOpenDiagnostics,
    onError,
  }: Props = $props();

  const transports: Option<Settings['dsdTransport']>[] = [
    {
      value: 'auto',
      label: 'Auto',
      hint: 'Native DSD when the DAC offers it, otherwise DoP.',
    },
    {
      value: 'native',
      label: 'Native',
      hint: 'Only native DSD; other DACs refuse DSD files.',
    },
    {
      value: 'dop',
      label: 'DoP',
      hint: 'DSD packed in PCM frames, for DACs that decode DoP.',
    },
  ];
  const fallbacks: Option<Settings['dsdFallback']>[] = [
    {
      value: 'refuse',
      label: 'Refuse',
      hint: 'The track does not play, and Lyra says why.',
    },
    {
      value: 'convert',
      label: 'Convert',
      hint: 'Plays as 24-bit PCM. Never bit-perfect, and labelled so.',
    },
  ];
  const gains: Option<number>[] = [
    {
      value: 0,
      label: '0 dB',
      hint: 'Never clips: peaks reach about −3 dBFS.',
    },
    {
      value: 3,
      label: '+3 dB',
      hint: 'Closer to PCM loudness; the loudest masters may clip.',
    },
    {
      value: 6,
      label: '+6 dB',
      hint: 'SACD reference level; loud masters clip.',
    },
  ];

  const delays: Option<number>[] = [
    { value: 0, label: 'Off', hint: 'Starts at once.' },
    { value: 250, label: '0.25 s', hint: 'For DACs that switch quickly.' },
    {
      value: 500,
      label: '0.5 s',
      hint: 'Enough for most DACs.',
    },
    { value: 1000, label: '1 s', hint: 'If the first notes are noise or cut.' },
    { value: 2000, label: '2 s', hint: 'For slow DACs.' },
  ];

  let settings = $state<Settings | null>(null);
  let saved = $state(false);

  const converting = $derived(
    settings?.dsdFallback === 'convert' && settings.dsdTransport === 'auto',
  );

  async function load() {
    settings = (await attempt(api.settings, onError)) ?? null;
  }

  async function save(change: Partial<Settings>) {
    const current = settings;

    if (current === null) return;

    saved = false;

    const next = await attempt(
      () => api.setSettings({ ...current, ...change }),
      onError,
    );

    if (next !== undefined) {
      settings = next;
      saved = true;
    }
  }

  function chooseTransport(value: Settings['dsdTransport']) {
    void save({ dsdTransport: value });
  }

  function chooseFallback(value: Settings['dsdFallback']) {
    void save({ dsdFallback: value });
  }

  function chooseDelay(value: number) {
    void save({ resyncDelayMs: value });
  }

  function chooseGain(value: number) {
    void save({ conversionGainDb: value });
  }

  /** The chosen option's line, shown under the setting's name. */
  function hint<T>(options: Option<T>[], value: T): string {
    return options.find((option) => option.value === value)?.hint ?? '';
  }

  $effect(() => {
    void load();
  });
</script>

<section class="settings" aria-labelledby="settings-title">
  <div class="heading">
    <h2 id="settings-title">Settings</h2>
    {#if saved}
      <span class="confirm" role="status"
        >Saved. Applies from the next track.</span
      >
    {/if}
  </div>

  <h3 class="group-title">Library</h3>
  <LibraryFolders {api} {onError} />

  <h3 class="group-title">Playback</h3>
  {#if settings}
    <div class="panel">
      <div class="setting">
        <div class="setting-text">
          <h4 id="transport-label">DSD transport</h4>
          <p class="muted">{hint(transports, settings.dsdTransport)}</p>
        </div>
        <Segmented
          labelledby="transport-label"
          name="dsd-transport"
          options={transports}
          value={settings.dsdTransport}
          onChoose={chooseTransport}
        />
      </div>
      <div class="setting">
        <div class="setting-text">
          <h4 id="delay-label">Silence before a format change</h4>
          <p class="muted">
            Lets the DAC switch rate or mode. {hint(
              delays,
              settings.resyncDelayMs,
            )}
          </p>
        </div>
        <Segmented
          labelledby="delay-label"
          name="resync-delay"
          options={delays}
          value={settings.resyncDelayMs}
          onChoose={chooseDelay}
        />
      </div>
      <div class="setting">
        <div class="setting-text">
          <h4 id="fallback-label">DSD on outputs without DSD</h4>
          <p class="muted">
            {hint(fallbacks, settings.dsdFallback)}
            {#if settings.dsdFallback === 'convert' && settings.dsdTransport !== 'auto'}
              Needs the Auto transport.
            {/if}
          </p>
        </div>
        <Segmented
          labelledby="fallback-label"
          name="dsd-fallback"
          options={fallbacks}
          value={settings.dsdFallback}
          onChoose={chooseFallback}
        />
      </div>
      {#if settings.dsdFallback === 'convert'}
        <div class="setting">
          <div class="setting-text">
            <h4 id="gain-label">Conversion gain</h4>
            <p class="muted">{hint(gains, settings.conversionGainDb)}</p>
          </div>
          <Segmented
            labelledby="gain-label"
            name="conversion-gain"
            options={gains}
            value={settings.conversionGainDb}
            disabled={!converting}
            onChoose={chooseGain}
          />
        </div>
      {/if}
    </div>
  {:else}
    <p class="muted">Loading settings…</p>
  {/if}

  <h3 class="group-title">Output</h3>
  <div class="panel">
    <div class="setting">
      <div class="setting-text">
        <h4>Diagnostics</h4>
        <p class="muted">
          {device === undefined
            ? 'Choose an output device first.'
            : `Buffer, dropouts and driver details for ${device.label}.`}
        </p>
      </div>
      <button
        type="button"
        class="small"
        disabled={device === undefined}
        onclick={onOpenDiagnostics}>Open</button
      >
    </div>
  </div>

  <h3 class="group-title">About</h3>
  <div class="panel about">
    <div class="setting">
      <div class="setting-text">
        <h4>{appInfo === null ? 'Lyra' : `Lyra ${appInfo.version}`}</h4>
        <p class="muted" data-testid="backend-status">
          {connected
            ? `Audio engine connected · ${appInfo?.platform ?? ''}`
            : 'Audio engine not connected'}
        </p>
      </div>
      <button type="button" class="small" disabled={checking} onclick={onCheck}
        >{checking ? 'Checking…' : 'Check'}</button
      >
    </div>
  </div>
</section>

<style>
  .settings {
    max-width: 760px;
  }
  .heading {
    justify-content: flex-start;
    margin-bottom: var(--space-5);
  }
  .heading h2 {
    font-size: var(--type-heading);
    line-height: var(--leading-heading);
    font-weight: var(--weight-strong);
  }
</style>

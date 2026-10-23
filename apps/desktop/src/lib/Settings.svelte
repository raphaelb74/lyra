<script lang="ts">
  import {
    attempt,
    type AppInfo,
    type Backend,
    type DeviceInfo,
    type Settings,
  } from './ipc';
  import Icon from './Icon.svelte';
  import LibraryFolders from './LibraryFolders.svelte';
  import ShortcutList from './ShortcutList.svelte';
  import Segmented from './Segmented.svelte';
  import type { Option } from './segmented';
  import { applyTheme, readTheme, THEMES, type Theme } from './theme';

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
    onOpenLicences: () => void;
    onOpenFixTags: () => void;
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
    onOpenLicences,
    onOpenFixTags,
    onError,
  }: Props = $props();

  let theme = $state(readTheme());

  const transports: Option<Settings['dsdTransport']>[] = [
    {
      value: 'auto',
      label: 'Auto',
      hint: 'Native DSD when available, otherwise DoP.',
    },
    {
      value: 'native',
      label: 'Native',
      hint: 'Native DSD only.',
    },
    {
      value: 'dop',
      label: 'DoP',
      hint: 'DSD carried in PCM frames.',
    },
  ];
  const dopChoices: Option<'off' | 'on'>[] = [
    {
      value: 'off',
      label: 'Off',
      hint: 'DoP only to DACs with native DSD.',
    },
    {
      value: 'on',
      label: 'On',
      hint: 'This DAC decodes DoP.',
    },
  ];
  const fallbacks: Option<Settings['dsdFallback']>[] = [
    {
      value: 'refuse',
      label: 'Refuse',
      hint: 'The track is skipped.',
    },
    {
      value: 'convert',
      label: 'Convert',
      hint: 'Plays as PCM. Not bit-perfect.',
    },
  ];
  const gains: Option<number>[] = [
    {
      value: 0,
      label: '0 dB',
      hint: 'Never clips.',
    },
    {
      value: 3,
      label: '+3 dB',
      hint: 'Louder. Hot masters may clip.',
    },
    {
      value: 6,
      label: '+6 dB',
      hint: 'SACD level. Loud masters clip.',
    },
  ];

  const delays: Option<number>[] = [
    { value: 0, label: 'Off', hint: 'Starts at once.' },
    { value: 250, label: '0.25 s', hint: 'For fast DACs.' },
    { value: 500, label: '0.5 s', hint: 'Suits most DACs.' },
    { value: 1000, label: '1 s', hint: 'If the first notes are cut.' },
    { value: 2000, label: '2 s', hint: 'For slow DACs.' },
  ];

  let settings = $state<Settings | null>(null);
  /** What the last save changed, said beside the title. */
  let saved = $state<string | null>(null);
  /** What turning online lookups on means, shown before it happens. */
  let disclosing = $state(false);
  const converting = $derived(
    settings?.dsdFallback === 'convert' && settings.dsdTransport === 'auto',
  );

  async function load() {
    settings = (await attempt(api.settings, onError)) ?? null;
  }

  async function save(
    change: Partial<Settings>,
    confirmation = 'Applies from the next track',
  ) {
    const current = settings;

    if (current === null) return;

    saved = null;

    const next = await attempt(
      () => api.setSettings({ ...current, ...change }),
      onError,
    );

    if (next !== undefined) {
      settings = next;
      saved = confirmation;
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

  /** Remembers whether the selected device decodes DoP. */
  function chooseDop(value: 'off' | 'on') {
    if (settings === null || device === undefined) return;

    const others = settings.dopDevices.filter((name) => name !== device.name);

    void save({
      dopDevices: value === 'on' ? [...others, device.name] : others,
    });
  }

  function setOnline(on: boolean) {
    disclosing = false;
    void save({ onlineLookups: on }, 'Saved.');
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
      <span class="confirm" role="status">{saved}</span>
    {/if}
  </div>

  <h3 class="group-title">Appearance</h3>
  <div class="panel">
    <div class="setting">
      <div class="setting-text">
        <h4 id="theme-label">Accent</h4>
      </div>
      <Segmented
        labelledby="theme-label"
        name="theme"
        options={THEMES}
        value={theme}
        onChoose={(choice: Theme) => {
          theme = choice;
          applyTheme(choice);
        }}
      />
    </div>
  </div>

  <h3 class="group-title">Library</h3>
  <LibraryFolders {api} {onError} />
  <div class="panel match">
    <div class="setting">
      <div class="setting-text">
        <h4>Fix tags</h4>
        <p class="muted">Tidy tags across the library.</p>
      </div>
      <button type="button" class="small with-icon" onclick={onOpenFixTags}
        ><Icon name="sparkles" size={14} />Review fixes…</button
      >
    </div>
    {#if settings}
      <div class="setting">
        <div class="setting-text">
          <h4>Online lookups</h4>
          <p class="muted">
            {settings.onlineLookups
              ? 'MusicBrainz, for ID Match.'
              : 'Lyra stays offline.'}
          </p>
        </div>
        {#if settings.onlineLookups}
          <button
            type="button"
            class="small"
            onclick={() => {
              setOnline(false);
            }}>Turn off</button
          >
        {:else if !disclosing}
          <button
            type="button"
            class="small"
            onclick={() => (disclosing = true)}>Turn on…</button
          >
        {/if}
      </div>
      {#if disclosing}
        <div class="disclosure" role="group" aria-labelledby="online-title">
          <h4 id="online-title">Before going online</h4>
          <ul>
            <li>Only when you open ID Match on an album.</li>
            <li>
              Sends its title and artist to MusicBrainz and the Cover Art
              Archive. Nothing about you, beyond your IP address.
            </li>
            <li>Nothing changes until you save. Your files stay untouched.</li>
          </ul>
          <div class="actions">
            <button
              type="button"
              class="small primary"
              onclick={() => {
                setOnline(true);
              }}>Turn on</button
            >
            <button
              type="button"
              class="small"
              onclick={() => (disclosing = false)}>Cancel</button
            >
          </div>
        </div>
      {/if}
    {/if}
  </div>

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
          <h4 id="delay-label">Resync delay</h4>
          <p class="muted">
            Silence while the DAC changes format. {hint(
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
          <h4 id="fallback-label">DSD on PCM-only DACs</h4>
          <p class="muted">
            {hint(fallbacks, settings.dsdFallback)}
            {#if settings.dsdFallback === 'convert' && settings.dsdTransport !== 'auto'}
              Needs Auto transport.
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
    {#if settings && device}
      {@const playsDop = settings.dopDevices.includes(device.name)
        ? 'on'
        : 'off'}
      <div class="setting">
        <div class="setting-text">
          <h4 id="dop-label">{device.label} plays DoP</h4>
          <p class="muted">{hint(dopChoices, playsDop)}</p>
        </div>
        <Segmented
          labelledby="dop-label"
          name="plays-dop"
          options={dopChoices}
          value={playsDop}
          onChoose={chooseDop}
        />
      </div>
    {/if}
    <div class="setting">
      <div class="setting-text">
        <h4>Diagnostics</h4>
        <p class="muted">
          {device === undefined
            ? 'Choose an output first.'
            : 'Buffer, dropouts and driver.'}
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

  <h3 class="group-title">Keyboard</h3>
  <ShortcutList />

  <h3 class="group-title">About</h3>
  <div class="panel about">
    <div class="setting">
      <div class="setting-text">
        <h4>{appInfo === null ? 'Lyra' : `Lyra ${appInfo.version}`}</h4>
        <p class="muted engine" data-testid="backend-status">
          {connected ? 'UNDA engine connected' : 'UNDA engine not connected'}
        </p>
      </div>
      <button type="button" class="small" disabled={checking} onclick={onCheck}
        >{checking ? 'Checking…' : 'Check'}</button
      >
    </div>
    <div class="setting">
      <div class="setting-text">
        <h4>Licences</h4>
        <p class="muted">Open-source components.</p>
      </div>
      <button type="button" class="small" onclick={onOpenLicences}>Open</button>
    </div>
  </div>
</section>

<style>
  .engine {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .engine::before {
    content: '';
    width: var(--space-5);
    height: var(--space-1);
    flex-shrink: 0;
    border-radius: var(--radius-s);
    background: var(--unda-spectrum);
  }
  .settings {
    max-width: 760px;
  }
  .match {
    margin-top: var(--space-3);
  }
  .disclosure {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3) 0 var(--space-4);
    border-top: 1px solid var(--line);
    font-size: var(--type-small);
  }
  .disclosure h4 {
    font-size: var(--type-body);
  }
  .disclosure ul {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding-left: var(--space-5);
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

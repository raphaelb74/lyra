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

  const transports: Option<Settings['dsdTransport']>[] = [
    {
      value: 'auto',
      label: 'Auto',
      hint: 'Native DSD when the DAC offers it, otherwise DoP if it plays DoP.',
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
  const dopChoices: Option<'off' | 'on'>[] = [
    {
      value: 'off',
      label: 'Off',
      hint: 'DoP goes only to DACs that also play native DSD. Turn on if the manual says this one supports DoP.',
    },
    {
      value: 'on',
      label: 'On',
      hint: 'This DAC receives DSD over DoP. A DAC without DoP would play it as noise.',
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
    confirmation = 'Saved. Applies from the next track.',
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

  <h3 class="group-title">Library</h3>
  <LibraryFolders {api} {onError} />
  <div class="panel match">
    <div class="setting">
      <div class="setting-text">
        <h4>Fix tags</h4>
        <p class="muted">Review messy tags across the library. Offline.</p>
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
              ? 'On: ID Match can ask MusicBrainz about an album you choose.'
              : 'Off: Lyra never goes online.'}
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
          <h4 id="online-title">Before ID Match goes online</h4>
          <ul>
            <li>
              Only when you open ID Match on an album: never at startup or
              during a scan.
            </li>
            <li>
              Lyra sends that album's title and artist, or the MusicBrainz ID it
              is tagged with, to musicbrainz.org, and asks coverartarchive.org
              for covers.
            </li>
            <li>
              Requests name Lyra, its version and a project contact address. No
              account, file name or anything else about you is sent. As with any
              website, those services see your IP address.
            </li>
            <li>
              Nothing changes until you save, and saved changes stay in Lyra;
              your music files are untouched.
            </li>
          </ul>
          <p class="muted">
            Data from MusicBrainz is public domain (CC0). Covers come from the
            Cover Art Archive and belong to their owners.
          </p>
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

  <h3 class="group-title">Keyboard</h3>
  <ShortcutList />

  <h3 class="group-title">About</h3>
  <div class="panel about">
    <div class="setting">
      <div class="setting-text">
        <h4>{appInfo === null ? 'Lyra' : `Lyra ${appInfo.version}`}</h4>
        <p class="muted" data-testid="backend-status">
          {connected
            ? `UNDA engine connected · ${appInfo?.platform ?? ''}`
            : 'UNDA engine not connected'}
        </p>
      </div>
      <button type="button" class="small" disabled={checking} onclick={onCheck}
        >{checking ? 'Checking…' : 'Check'}</button
      >
    </div>
    <div class="setting">
      <div class="setting-text">
        <h4>Licences</h4>
        <p class="muted">The open-source components Lyra is built on.</p>
      </div>
      <button type="button" class="small" onclick={onOpenLicences}>Open</button>
    </div>
  </div>
</section>

<style>
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

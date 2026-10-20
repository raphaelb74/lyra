<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  import {
    BackendError,
    backend,
    onFilesDropped,
    onStatus,
    type DeviceInfo,
    type DroppedFile,
    type PlayerStatus,
  } from './lib/ipc';
  import PlayerBar from './lib/PlayerBar.svelte';

  const api = backend();
  const laterViews = ['Albums', 'Artists', 'Playlists', 'Settings'];

  let connection = $state('Connecting to Rust…');
  let connected = $state(false);
  let checking = $state(false);
  let devices = $state<DeviceInfo[]>([]);
  let devicesLoaded = $state(false);
  let selectedDevice = $state('');
  let files = $state<DroppedFile[]>([]);
  let dropNotice = $state<string | null>(null);
  let status = $state<PlayerStatus>({
    state: 'stopped',
    track: null,
    error: null,
  });
  let position = $state(0);
  let actionError = $state<string | null>(null);

  const device = $derived(devices.find((d) => d.name === selectedDevice));
  const shownError = $derived(actionError ?? status.error?.message ?? null);

  const UNEXPECTED = 'Unexpected error while contacting Rust.';

  async function checkBackend() {
    checking = true;
    connected = false;

    try {
      const info = await api.appInfo();

      connection = `${info.name} ${info.version} · Rust connected · ${info.platform}`;
      connected = true;
    } catch (error) {
      connection = error instanceof BackendError ? error.message : UNEXPECTED;
    } finally {
      checking = false;
    }
  }

  async function loadDevices() {
    try {
      devices = await api.listDevices();

      if (!devices.some((d) => d.name === selectedDevice)) {
        // Prefer an external DAC; never switch while one is chosen.
        selectedDevice = (devices.find((d) => d.usb) ?? devices[0])?.name ?? '';
      }
    } catch (error) {
      actionError = error instanceof BackendError ? error.message : UNEXPECTED;
    } finally {
      devicesLoaded = true;
    }
  }

  /** Runs a command; shows its error instead of throwing. */
  async function attempt(action: () => Promise<null>) {
    actionError = null;

    try {
      await action();
    } catch (error) {
      actionError = error instanceof BackendError ? error.message : UNEXPECTED;
    }
  }

  function playFile(file: DroppedFile) {
    void attempt(() => api.play(file.path, selectedDevice));
  }

  function toggle() {
    void attempt(() =>
      status.state === 'playing' ? api.pause() : api.resume(),
    );
  }

  onMount(() => {
    void checkBackend();

    if (!isTauri()) return;

    void loadDevices();
    void api.status().then((current) => (status = current));

    const unlisten = [
      onStatus((next) => {
        status = next;
      }),
      onFilesDropped((dropped) => {
        dropNotice =
          dropped.length === 0
            ? 'Only FLAC and WAV files can be played.'
            : null;
        const known = new Set(files.map((f) => f.path));

        files = [...files, ...dropped.filter((f) => !known.has(f.path))];
      }),
    ];

    return () => {
      for (const pending of unlisten) {
        void pending.then((stop) => {
          stop();
        });
      }
    };
  });

  // Poll the position only while audio moves.
  $effect(() => {
    if (status.state !== 'playing') return;

    const timer = setInterval(() => {
      void api.position().then((seconds) => (position = seconds));
    }, 250);

    return () => {
      clearInterval(timer);
    };
  });
</script>

<svelte:head><title>Lyra — Music player</title></svelte:head>

<div class="app">
  <div class="layout">
    <aside aria-label="Application">
      <h1><span aria-hidden="true">♮</span> Lyra</h1>
      <nav aria-label="Main navigation">
        <button type="button" aria-current="page">Now playing</button>
        {#each laterViews as view (view)}
          <button type="button" disabled title="Not available yet"
            >{view}</button
          >
        {/each}
      </nav>
      <small>Local music. Clear intent.</small>
    </aside>
    <main>
      <header>
        <div>
          <p class="eyebrow">DIRECT TO YOUR DAC</p>
          <h2>A little room for listening.</h2>
          <p class="muted">
            Exact samples, exact rate, straight to the hardware.
          </p>
        </div>
        <span class="badge">M1 · Preview</span>
      </header>
      <section class="backend" aria-label="Backend connection">
        <div>
          <h3>Desktop connection</h3>
          <p
            role="status"
            data-testid="backend-status"
            data-connected={connected}
          >
            {connection}
          </p>
        </div>
        <button onclick={checkBackend} disabled={checking}
          >{checking ? 'Checking…' : 'Check backend'}</button
        >
      </section>
      {#if shownError}
        <p class="alert" role="alert">{shownError}</p>
      {/if}
      <section aria-labelledby="device-title">
        <div class="section-heading">
          <h3 id="device-title">Output device</h3>
          <button type="button" onclick={loadDevices}>Refresh</button>
        </div>
        <div class="device" data-testid="devices" data-loaded={devicesLoaded}>
          {#if devices.length > 0}
            <select bind:value={selectedDevice} aria-label="Output device">
              {#each devices as option (option.name)}
                <option value={option.name}
                  >{option.cardName} · {option.pcmName}</option
                >
              {/each}
            </select>
            <small class="muted">{selectedDevice}</small>
          {:else}
            <p class="muted">
              {devicesLoaded
                ? 'No audio devices found.'
                : 'Looking for devices…'}
            </p>
          {/if}
        </div>
      </section>
      <section aria-labelledby="files-title">
        <div class="section-heading">
          <h3 id="files-title">Files</h3>
          <span class="muted">{files.length} dropped</span>
        </div>
        {#if files.length > 0}
          <ul class="files">
            {#each files as file (file.path)}
              <li>
                <button
                  type="button"
                  aria-current={status.track?.fileName === file.name}
                  disabled={selectedDevice === ''}
                  onclick={() => {
                    playFile(file);
                  }}><span>{file.name}</span><small>{file.path}</small></button
                >
              </li>
            {/each}
          </ul>
        {/if}
        <p class="drop">
          {dropNotice ??
            'Drop FLAC or WAV files anywhere on this window, then choose one to play.'}
        </p>
      </section>
    </main>
  </div>
  <PlayerBar
    {status}
    {position}
    {device}
    onToggle={toggle}
    onStop={() => {
      void attempt(api.stop);
    }}
    onSeek={(seconds: number) => {
      position = seconds;
      void attempt(() => api.seek(seconds));
    }}
  />
</div>

<style>
  .app {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    height: 100dvh;
  }
  .layout {
    display: flex;
    min-height: 0;
  }
  aside {
    width: 215px;
    flex-shrink: 0;
    background: #151c24;
    padding: 28px 24px;
    border-right: 1px solid #29323a;
    display: flex;
    flex-direction: column;
  }
  h1 {
    font-size: 25px;
    letter-spacing: 4px;
    text-transform: uppercase;
    margin: 8px 0 48px;
  }
  h1 span {
    color: #b4d9cb;
  }
  nav {
    display: grid;
    gap: 8px;
    color: #a2adb7;
  }
  nav button {
    border: 0;
    background: transparent;
    padding: 12px 16px;
    text-align: left;
  }
  nav button:disabled {
    opacity: 1;
  }
  nav [aria-current='page'] {
    color: #d9eee4;
    background: #263831;
  }
  aside > small {
    margin-top: auto;
    padding-top: 32px;
    color: #a2adb7;
  }
  main {
    flex: 1;
    padding: 40px 48px;
    min-width: 0;
    max-width: 1400px;
    overflow-y: auto;
  }
  header,
  .section-heading,
  .backend {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
  }
  .eyebrow {
    color: #b4d9cb;
    font-size: 11px;
    letter-spacing: 2.5px;
  }
  h2 {
    font-weight: 500;
    font-size: clamp(24px, 3vw, 36px);
    margin: 16px 0;
    letter-spacing: -1px;
  }
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  .badge {
    color: #b4d9cb;
    border: 1px solid #38534a;
    padding: 7px 10px;
    border-radius: 6px;
    white-space: nowrap;
    font-size: 12px;
  }
  section {
    margin-top: 32px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  .section-heading {
    margin-bottom: 20px;
  }
  .backend {
    border-block: 1px solid #29343d;
    padding-block: 16px;
  }
  .files {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 8px;
  }
  .files button {
    width: 100%;
    text-align: left;
    display: flex;
    justify-content: space-between;
    gap: 16px;
    background: #182129;
    border-color: #29343d;
  }
  .files button[aria-current='true'] {
    border-color: #577369;
    background: #1f2d2a;
  }
  .files small {
    color: #a2adb7;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drop {
    border: 1px dashed #38534a;
    border-radius: 8px;
    padding: 28px;
    text-align: center;
    color: #a2adb7;
    font-size: 13px;
  }
  .device {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  select {
    font: inherit;
    color: inherit;
    background: #182129;
    border: 1px solid #577369;
    border-radius: 8px;
    padding: 10px 12px;
    min-width: min(100%, 420px);
  }
  .alert {
    border: 1px solid #8a5a3c;
    background: #2a1f1a;
    color: #f0d2bf;
    border-radius: 8px;
    padding: 12px 16px;
    font-size: 13px;
    line-height: 1.5;
  }
  .backend p {
    font-size: 12px;
    color: #b4d9cb;
    line-height: 1.5;
    margin-bottom: 0;
  }
  @media (max-width: 950px) {
    main {
      padding: 30px 25px;
    }
    aside {
      width: 170px;
      padding: 25px 15px;
    }
  }
  @media (max-width: 650px) {
    aside {
      display: none;
    }
    header {
      align-items: flex-start;
    }
    .badge {
      display: none;
    }
    .backend {
      align-items: flex-start;
    }
  }
</style>

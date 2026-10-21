<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  import {
    BackendError,
    attempt,
    backend,
    onFilesDropped,
    onStatus,
    type DeviceInfo,
    type DroppedFile,
    type PlayerStatus,
  } from './lib/ipc';
  import Library from './lib/Library.svelte';
  import PlayerBar from './lib/PlayerBar.svelte';

  const api = backend();
  const laterViews = ['Artists', 'Playlists', 'Settings'];

  type View = 'library' | 'files';

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
  let view = $state<View>('library');

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

  function showError(message: string) {
    actionError = message;
  }

  /** Runs a command; shows its error instead of throwing. */
  async function command(action: () => Promise<null>) {
    actionError = null;
    await attempt(action, showError);
  }

  function playFile(file: DroppedFile) {
    void command(() => api.play(file.path, selectedDevice));
  }

  function toggle() {
    void command(() =>
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
        view = 'files';
        dropNotice =
          dropped.length === 0
            ? 'Only FLAC, WAV, DSF and DFF files can be played.'
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
        <button
          type="button"
          aria-current={view === 'library' ? 'page' : undefined}
          onclick={() => (view = 'library')}>Library</button
        >
        <button
          type="button"
          aria-current={view === 'files' ? 'page' : undefined}
          onclick={() => (view = 'files')}>Files</button
        >
        {#each laterViews as later (later)}
          <button type="button" disabled title="Not available yet"
            >{later}</button
          >
        {/each}
      </nav>
      <section class="output" aria-labelledby="device-title">
        <div class="section-heading">
          <h3 id="device-title">Output</h3>
          <button type="button" class="small" onclick={loadDevices}
            >Refresh</button
          >
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
      <section class="backend" aria-label="Backend connection">
        <p
          role="status"
          data-testid="backend-status"
          data-connected={connected}
        >
          {connection}
        </p>
        <button
          type="button"
          class="small"
          onclick={checkBackend}
          disabled={checking}>{checking ? 'Checking…' : 'Check'}</button
        >
      </section>
    </aside>
    <main>
      {#if shownError}
        <p class="alert" role="alert">{shownError}</p>
      {/if}
      {#if view === 'library'}
        <Library {api} device={selectedDevice} onError={showError} />
      {:else}
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
                    }}
                    ><span>{file.name}</span><small>{file.path}</small></button
                  >
                </li>
              {/each}
            </ul>
          {/if}
          <p class="drop">
            {dropNotice ??
              'Drop FLAC, WAV, DSF or DFF files anywhere on this window to play them, or a folder to add it to the library.'}
          </p>
        </section>
      {/if}
    </main>
  </div>
  <PlayerBar
    {status}
    {position}
    {device}
    onToggle={toggle}
    onStop={() => {
      void command(api.stop);
    }}
    onSeek={(seconds: number) => {
      position = seconds;
      void command(() => api.seek(seconds));
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
    width: 240px;
    flex-shrink: 0;
    background: #151c24;
    padding: 28px 20px;
    border-right: 1px solid #29323a;
    display: flex;
    flex-direction: column;
    gap: 24px;
    overflow-y: auto;
  }
  h1 {
    font-size: 25px;
    letter-spacing: 4px;
    text-transform: uppercase;
    margin: 8px 0 24px;
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
    opacity: 0.55;
  }
  nav [aria-current='page'] {
    color: #d9eee4;
    background: #263831;
  }
  .output {
    margin-top: auto;
  }
  main {
    flex: 1;
    padding: 40px 48px;
    min-width: 0;
    max-width: 1400px;
    overflow-y: auto;
  }
  .section-heading,
  .backend {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  .section-heading {
    margin-bottom: 12px;
  }
  main .section-heading {
    margin-bottom: 20px;
  }
  .small {
    padding: 4px 10px;
    font-size: 12px;
  }
  .backend {
    border-top: 1px solid #29343d;
    padding-top: 16px;
  }
  .backend p {
    font-size: 11px;
    color: #b4d9cb;
    line-height: 1.5;
    margin: 0;
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
    display: grid;
    gap: 6px;
  }
  .device small {
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  select {
    font: inherit;
    font-size: 13px;
    color: inherit;
    background: #182129;
    border: 1px solid #577369;
    border-radius: 8px;
    padding: 8px 10px;
    width: 100%;
  }
  .alert {
    border: 1px solid #8a5a3c;
    background: #2a1f1a;
    color: #f0d2bf;
    border-radius: 8px;
    padding: 12px 16px;
    font-size: 13px;
    line-height: 1.5;
    margin-top: 0;
  }
  @media (max-width: 950px) {
    main {
      padding: 30px 25px;
    }
    aside {
      width: 200px;
      padding: 25px 15px;
    }
  }
  @media (max-width: 650px) {
    aside {
      display: none;
    }
  }
</style>

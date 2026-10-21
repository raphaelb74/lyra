<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  import {
    BackendError,
    attempt,
    backend,
    type DeviceInfo,
    type PlayerStatus,
    type TrackDetails,
  } from './lib/ipc';
  import { onStatus } from './lib/events';
  import Library from './lib/Library.svelte';
  import NowPlaying from './lib/NowPlaying.svelte';
  import PlayerBar from './lib/PlayerBar.svelte';
  import Playlists from './lib/Playlists.svelte';
  import QueueView from './lib/QueueView.svelte';
  import Diagnostics from './lib/Diagnostics.svelte';
  import Settings from './lib/Settings.svelte';
  import TitleBar from './lib/TitleBar.svelte';
  import DevicePicker from './lib/DevicePicker.svelte';
  import VolumeControl from './lib/VolumeControl.svelte';

  const api = backend();

  type View =
    'library' | 'playlists' | 'now' | 'queue' | 'settings' | 'diagnostics';

  const views: readonly { id: View; label: string }[] = [
    { id: 'library', label: 'Library' },
    { id: 'playlists', label: 'Playlists' },
    { id: 'now', label: 'Now playing' },
    { id: 'queue', label: 'Queue' },
    { id: 'settings', label: 'Settings' },
    { id: 'diagnostics', label: 'Diagnostics' },
  ];

  let connection = $state('Connecting to Rust…');
  let connected = $state(false);
  let checking = $state(false);
  let devices = $state<DeviceInfo[]>([]);
  let devicesLoaded = $state(false);
  let selectedDevice = $state('');
  let status = $state<PlayerStatus>({
    state: 'stopped',
    track: null,
    error: null,
    queueLength: 0,
  });
  let position = $state(0);
  let actionError = $state<string | null>(null);
  let view = $state<View>('library');
  let details = $state<TrackDetails | null>(null);

  const device = $derived(devices.find((d) => d.name === selectedDevice));
  const libraryId = $derived(status.track?.libraryId ?? null);
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
        // The list comes most-wanted first (USB DACs, then built-in
        // audio); never switch while a listed device is chosen.
        selectedDevice = devices[0]?.name ?? '';
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
    ];

    return () => {
      for (const pending of unlisten) {
        void pending.then((stop) => {
          stop();
        });
      }
    };
  });

  // Library data for the playing track (title, album, cover).
  $effect(() => {
    const id = libraryId;

    if (id === null) {
      details = null;

      return;
    }

    void attempt(() => api.trackDetails(id), showError).then((found) => {
      if (libraryId === id) details = found ?? null;
    });
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
  <TitleBar />
  <div class="layout">
    <aside aria-label="Application">
      <nav aria-label="Main navigation">
        {#each views as item (item.id)}
          <button
            type="button"
            aria-current={view === item.id ? 'page' : undefined}
            onclick={() => (view = item.id)}>{item.label}</button
          >
        {/each}
      </nav>
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
        <Library
          {api}
          device={selectedDevice}
          playingId={libraryId}
          onError={showError}
        />
      {:else if view === 'playlists'}
        <Playlists
          {api}
          device={selectedDevice}
          playingId={libraryId}
          onError={showError}
        />
      {:else if view === 'now'}
        <NowPlaying {status} {details} {device} />
      {:else if view === 'settings'}
        <Settings {api} onError={showError} />
      {:else if view === 'diagnostics'}
        <Diagnostics {api} {device} {status} onError={showError} />
      {:else}
        <QueueView
          {api}
          current={status.track?.queueIndex ?? null}
          queueLength={status.queueLength}
          onError={showError}
        />
      {/if}
    </main>
  </div>
  <PlayerBar
    {status}
    {position}
    {details}
    onToggle={toggle}
    onPrevious={() => {
      void command(api.previous);
    }}
    onNext={() => {
      void command(api.next);
    }}
    onStop={() => {
      void command(api.stop);
    }}
    onSeek={(seconds: number) => {
      position = seconds;
      void command(() => api.seek(seconds));
    }}
  >
    {#snippet output()}
      {#if selectedDevice !== ''}
        <VolumeControl
          {api}
          device={selectedDevice}
          {status}
          onError={showError}
        />
      {/if}
      <DevicePicker
        {devices}
        selected={selectedDevice}
        loaded={devicesLoaded}
        onSelect={(name: string) => (selectedDevice = name)}
        onRefresh={loadDevices}
      />
    {/snippet}
  </PlayerBar>
</div>

<style>
  .app {
    --sidebar-width: 232px;

    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    height: 100dvh;
  }
  /* No system frame: a hairline marks the window's edge. */
  .app::after {
    content: '';
    position: fixed;
    inset: 0;
    border: 1px solid var(--line);
    pointer-events: none;
    z-index: 100;
  }
  .layout {
    display: flex;
    min-height: 0;
  }
  aside {
    width: var(--sidebar-width);
    flex-shrink: 0;
    background: var(--surface);
    padding: var(--space-3) var(--space-4) var(--space-4);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    overflow: hidden auto;
  }
  nav {
    display: grid;
    gap: var(--space-0);
  }
  nav button {
    border: 0;
    padding: var(--space-2) var(--space-3);
    text-align: left;
    color: var(--text-secondary);
  }
  nav button:hover:not(:disabled) {
    color: var(--text);
  }
  nav [aria-current='page'] {
    color: var(--text);
    background: var(--surface-selected);
  }
  main {
    flex: 1;
    padding: var(--space-2) var(--space-7) var(--space-6);
    min-width: 0;
    max-width: 1400px;
    overflow-y: auto;
  }
  .backend {
    margin-top: auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    border-top: 1px solid var(--line);
    padding-top: var(--space-4);
  }
  .backend p {
    font-size: var(--type-caption);
    line-height: var(--leading-caption);
    color: var(--text-muted);
  }
  @media (max-width: 950px) {
    main {
      padding: var(--space-2) var(--space-5) var(--space-6);
    }
    .app {
      --sidebar-width: 200px;
    }
    aside {
      padding: var(--space-3);
    }
  }
  @media (max-width: 650px) {
    aside {
      display: none;
    }
  }
</style>

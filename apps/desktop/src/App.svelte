<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { cubicOut } from 'svelte/easing';
  import { slide } from 'svelte/transition';

  import {
    BackendError,
    attempt,
    backend,
    type AppInfo,
    type DeviceInfo,
    type PlayerStatus,
    type Repeat,
    type TrackDetails,
  } from './lib/ipc';
  import { onStatus } from './lib/events';
  import Library from './lib/Library.svelte';
  import NowPlaying from './lib/NowPlaying.svelte';
  import PlayerBar from './lib/PlayerBar.svelte';
  import Playlists from './lib/Playlists.svelte';
  import QueuePanel from './lib/QueuePanel.svelte';
  import Diagnostics from './lib/Diagnostics.svelte';
  import Settings from './lib/Settings.svelte';
  import type { IconName } from './assets/icons/lucide';
  import ContextMenu from './lib/ContextMenu.svelte';
  import Icon from './lib/Icon.svelte';
  import { actionFor, isTyping, type Action } from './lib/shortcuts';
  import TitleBar from './lib/TitleBar.svelte';
  import DevicePicker from './lib/DevicePicker.svelte';
  import VolumeControl from './lib/VolumeControl.svelte';
  import { readFlag, writeFlag } from './lib/storage';

  const api = backend();

  type View =
    | 'albums'
    | 'artists'
    | 'tracks'
    | 'playlists'
    | 'settings'
    /* Opened from the player bar and from Settings, not the sidebar. */
    | 'now'
    | 'diagnostics';

  const views: readonly { id: View; label: string; icon: IconName }[] = [
    { id: 'albums', label: 'Albums', icon: 'disc-3' },
    { id: 'artists', label: 'Artists', icon: 'mic-vocal' },
    { id: 'tracks', label: 'Tracks', icon: 'audio-lines' },
    { id: 'playlists', label: 'Playlists', icon: 'list-music' },
  ];

  const QUEUE_OPEN_KEY = 'lyra.queue.open';

  let appInfo = $state<AppInfo | null>(null);
  /** Why the audio engine cannot be reached; null while it can. */
  let connectionError = $state<string | null>(null);
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
    shuffle: false,
    repeat: 'off',
    queueRevision: 0,
    hasNext: false,
  });
  let position = $state(0);
  let actionError = $state<string | null>(null);
  let view = $state<View>('albums');
  /** Where Now Playing and Diagnostics go back to. */
  let back = $state<View>('albums');
  let details = $state<TrackDetails | null>(null);
  /** The queue pane on the right, remembered between launches. */
  let queueOpen = $state(readFlag(QUEUE_OPEN_KEY));

  const device = $derived(devices.find((d) => d.name === selectedDevice));
  const libraryId = $derived(status.track?.libraryId ?? null);
  const shownError = $derived(actionError ?? status.error?.message ?? null);

  const UNEXPECTED = 'Lyra cannot reach its audio engine.';

  async function checkBackend() {
    checking = true;
    connected = false;

    try {
      appInfo = await api.appInfo();
      connected = true;
      connectionError = null;
    } catch (error) {
      connectionError =
        error instanceof BackendError ? error.message : UNEXPECTED;
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

  function setQueueOpen(open: boolean) {
    queueOpen = open;
    writeFlag(QUEUE_OPEN_KEY, open);
  }

  /** The panel slide follows --dur-panel, which is 0 with reduced motion. */
  function panelMotion() {
    const token = getComputedStyle(document.documentElement)
      .getPropertyValue('--dur-panel')
      .trim();

    const amount = parseFloat(token) || 0;

    return {
      axis: 'x' as const,
      duration: token.endsWith('ms') ? amount : amount * 1000,
      easing: cubicOut,
    };
  }

  /** Opens a view that has a way back (Now Playing, Diagnostics). */
  function visit(next: View) {
    if (view !== 'now' && view !== 'diagnostics') back = view;

    view = next;
  }

  /** Seconds a Shift+arrow press moves the playhead. */
  const SEEK_STEP = 10;

  function seekBy(seconds: number) {
    const duration = status.track?.durationSeconds ?? 0;

    if (duration === 0) return;

    const target = Math.min(duration, Math.max(0, position + seconds));

    position = target;
    void command(() => api.seek(target));
  }

  async function focusSearch() {
    view = 'tracks';
    await tick();
    document.querySelector<HTMLInputElement>('input[type=search]')?.focus();
  }

  function run(action: Action) {
    const active = status.state === 'playing' || status.state === 'paused';

    switch (action) {
      case 'toggle':
        if (active) toggle();
        break;
      case 'next':
        void command(api.next);
        break;
      case 'previous':
        void command(api.previous);
        break;
      case 'stop':
        if (active) void command(api.stop);
        break;
      case 'seekForward':
        seekBy(SEEK_STEP);
        break;
      case 'seekBack':
        seekBy(-SEEK_STEP);
        break;
      case 'search':
        void focusSearch();
        break;
      case 'ignore':
        break;
      case 'queue':
        setQueueOpen(!queueOpen);
        break;
      default:
        view = action;
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.defaultPrevented) return;

    if (event.key === 'Escape') {
      if (view === 'now' || view === 'diagnostics') view = back;
      else if (queueOpen) setQueueOpen(false);

      return;
    }

    if (isTyping(event.target)) return;

    const action = actionFor(event);

    if (action === null) return;

    // Space plays or pauses even on a focused button or slider.
    event.preventDefault();
    run(action);
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
<svelte:window onkeydown={onKey} />

<div class="app" data-connected={connected}>
  <TitleBar />
  <ContextMenu />
  <div class="layout">
    <aside aria-label="Application">
      <nav aria-label="Main navigation">
        {#each views as item (item.id)}
          <button
            type="button"
            aria-current={view === item.id ? 'page' : undefined}
            title={item.label}
            onclick={() => (view = item.id)}
          >
            <Icon name={item.icon} size={18} />
            <span class="label">{item.label}</span>
          </button>
        {/each}
      </nav>
      <nav class="end" aria-label="Preferences">
        <button
          type="button"
          aria-current={view === 'settings' || view === 'diagnostics'
            ? 'page'
            : undefined}
          title="Settings"
          onclick={() => (view = 'settings')}
        >
          <Icon name="settings" size={18} />
          <span class="label">Settings</span>
        </button>
      </nav>
    </aside>
    <main>
      {#if connectionError}
        <div class="alert engine" role="alert">
          <span>{connectionError}</span>
          <button
            type="button"
            class="small"
            disabled={checking}
            onclick={checkBackend}>{checking ? 'Retrying…' : 'Retry'}</button
          >
        </div>
      {/if}
      {#if shownError}
        <p class="alert" role="alert">{shownError}</p>
      {/if}
      {#if view === 'albums' || view === 'artists' || view === 'tracks'}
        <Library
          section={view}
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
        <NowPlaying {status} {details} {device} onBack={() => (view = back)} />
      {:else if view === 'settings'}
        <Settings
          {api}
          {appInfo}
          {connected}
          {checking}
          {device}
          onCheck={checkBackend}
          onOpenDiagnostics={() => {
            visit('diagnostics');
          }}
          onError={showError}
        />
      {:else}
        <Diagnostics
          {api}
          {device}
          {status}
          onBack={() => (view = 'settings')}
          onError={showError}
        />
      {/if}
    </main>
    {#if queueOpen}
      <div class="queue-pane" transition:slide={panelMotion()}>
        <QueuePanel
          {api}
          {status}
          device={selectedDevice}
          onClose={() => {
            setQueueOpen(false);
          }}
          onError={showError}
        />
      </div>
    {/if}
  </div>
  <PlayerBar
    {status}
    {position}
    {details}
    onOpen={() => {
      visit('now');
    }}
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
    onShuffle={(on: boolean) => {
      void command(() => api.setShuffle(on));
    }}
    onRepeat={(mode: Repeat) => {
      void command(() => api.setRepeat(mode));
    }}
    onSeek={(seconds: number) => {
      position = seconds;
      void command(() => api.seek(seconds));
    }}
  >
    {#snippet output()}
      <button
        type="button"
        class="icon"
        aria-label="Queue"
        aria-pressed={queueOpen}
        title="Queue (Ctrl+5)"
        onclick={() => {
          setQueueOpen(!queueOpen);
        }}><Icon name="list-ordered" size={18} /></button
      >
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
    /* Never wider than the window, whatever a view holds. */
    grid-template-columns: minmax(0, 1fr);
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
  nav.end {
    margin-top: auto;
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    border: 0;
    padding: var(--space-2) var(--space-3);
    text-align: left;
    color: var(--text-secondary);
    font-weight: var(--weight-medium);
  }
  nav button:hover:not(:disabled) {
    color: var(--text);
  }
  nav [aria-current='page'] {
    color: var(--text);
    background: var(--surface-selected);
  }
  nav [aria-current='page'] :global(svg) {
    color: var(--accent);
  }
  .label {
    flex: 1;
  }
  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    /* Side margin of every view; a view that scrolls on its own reaches
       past it to the window's edge (Artists). */
    --page-gutter: var(--space-7);

    padding: var(--space-2) var(--page-gutter) var(--space-6);
    min-width: 0;
    max-width: 1400px;
    overflow-y: auto;
  }
  /* Views flow top to bottom; one may fill the height (Artists). */
  main > :global(*) {
    flex-shrink: 0;
  }
  /* Fixed inner width: the pane slides in without its text reflowing. */
  .queue-pane {
    --queue-width: 340px;

    flex-shrink: 0;
    overflow: hidden;
  }
  .engine {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }
  /* Narrow windows: an icon rail; names stay for screen readers and in
     tooltips. */
  @media (max-width: 1000px) {
    .app {
      --sidebar-width: 64px;
    }
    aside {
      padding: var(--space-3) var(--space-2);
    }
    nav button {
      justify-content: center;
      position: relative;
    }
    .label {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    main {
      --page-gutter: var(--space-5);
    }
  }
</style>

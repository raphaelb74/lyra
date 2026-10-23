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
  import { onLibraryChanged, onStatus } from './lib/events';
  import Library from './lib/Library.svelte';
  import NowPlaying from './lib/NowPlaying.svelte';
  import PlayerBar from './lib/PlayerBar.svelte';
  import Playlists from './lib/Playlists.svelte';
  import QueuePanel from './lib/QueuePanel.svelte';
  import Diagnostics from './lib/Diagnostics.svelte';
  import FixTags from './lib/FixTags.svelte';
  import Licences from './lib/Licences.svelte';
  import Settings from './lib/Settings.svelte';
  import SignalPath from './lib/SignalPath.svelte';
  import type { IconName } from './assets/icons/lucide';
  import ContextMenu from './lib/ContextMenu.svelte';
  import Icon from './lib/Icon.svelte';
  import { actionFor, isTyping, type Action } from './lib/shortcuts';
  import TitleBar from './lib/TitleBar.svelte';
  import DevicePicker from './lib/DevicePicker.svelte';
  import VolumeControl from './lib/VolumeControl.svelte';
  import { readFlag, writeFlag } from './lib/storage';
  import Toast from './lib/Toast.svelte';
  import { dismissToast } from './lib/toast.svelte';

  const api = backend();

  type View =
    | 'albums'
    | 'artists'
    | 'tracks'
    | 'playlists'
    | 'settings'
    /* Opened from the player bar and from Settings, not the sidebar. */
    | 'now'
    | 'diagnostics'
    | 'licences'
    | 'fixtags';

  type Section = 'albums' | 'artists' | 'tracks';

  function isSection(candidate: View): candidate is Section {
    return (
      candidate === 'albums' ||
      candidate === 'artists' ||
      candidate === 'tracks'
    );
  }

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
  /** The library stays mounted behind other views, on the section last
   * shown, so its open album and filters survive a trip to Settings. */
  let lastSection: Section = 'albums';
  const section = $derived.by(() => {
    if (isSection(view)) lastSection = view;

    return lastSection;
  });
  let mainElement = $state<HTMLElement>();
  /** How far each view was scrolled, restored when it comes back. */
  const scrolled: Partial<Record<View, number>> = {};

  // A new view: back where it was left, and no notice from the old one.
  $effect(() => {
    if (mainElement) mainElement.scrollTop = scrolled[view] ?? 0;

    dismissToast();
  });
  let details = $state<TrackDetails | null>(null);
  /** Bumped when the library changes: an edit may rename what plays. */
  let libraryVersion = $state(0);
  /** The queue pane on the right, remembered between launches. */
  let queueOpen = $state(readFlag(QUEUE_OPEN_KEY));

  const device = $derived(devices.find((d) => d.name === selectedDevice));
  /** The device the track plays on, which may not be the selected one. */
  const playingDevice = $derived(
    devices.find((d) => d.name === status.track?.device),
  );
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

  /** Views with a way back instead of a sidebar entry. */
  const DETAIL: readonly View[] = ['now', 'diagnostics', 'licences', 'fixtags'];

  /** Opens a view that has a way back (Now Playing, Diagnostics, Licences). */
  function visit(next: View) {
    if (!DETAIL.includes(view)) back = view;

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
      if (DETAIL.includes(view)) view = back;
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
      onLibraryChanged(() => {
        libraryVersion += 1;
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
    const version = libraryVersion;

    if (id === null) {
      details = null;

      return;
    }

    void attempt(() => api.trackDetails(id), showError).then((found) => {
      if (libraryId === id && libraryVersion === version) {
        details = found ?? null;
      }
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
          aria-current={view === 'settings' ||
          view === 'diagnostics' ||
          view === 'licences' ||
          view === 'fixtags'
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
    <div class="stage">
      <main
        bind:this={mainElement}
        onscroll={(event) => {
          scrolled[view] = event.currentTarget.scrollTop;
        }}
      >
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
        <div class="keep" hidden={!isSection(view)}>
          <Library
            {section}
            {api}
            device={selectedDevice}
            playingId={libraryId}
            onError={showError}
          />
        </div>
        {#if view === 'playlists'}
          <Playlists
            {api}
            device={selectedDevice}
            playingId={libraryId}
            onError={showError}
          />
        {:else if view === 'now'}
          <NowPlaying
            {status}
            {details}
            device={playingDevice}
            onBack={() => (view = back)}
          />
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
            onOpenLicences={() => {
              visit('licences');
            }}
            onOpenFixTags={() => {
              visit('fixtags');
            }}
            onError={showError}
          />
        {:else if view === 'fixtags'}
          <FixTags
            {api}
            albumId={null}
            scope="Whole library"
            backLabel="Settings"
            onBack={() => (view = 'settings')}
            onError={showError}
          />
        {:else if view === 'licences'}
          <Licences onBack={() => (view = 'settings')} />
        {:else if view === 'diagnostics'}
          <Diagnostics
            {api}
            {device}
            {status}
            onBack={() => (view = 'settings')}
            onError={showError}
          />
        {/if}
      </main>
      <Toast />
    </div>
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
    {#snippet volume()}
      {#if selectedDevice !== ''}
        <VolumeControl
          {api}
          device={selectedDevice}
          {status}
          onError={showError}
        />
      {/if}
    {/snippet}
    {#snippet output()}
      <DevicePicker
        {devices}
        selected={selectedDevice}
        loaded={devicesLoaded}
        onSelect={(name: string) => (selectedDevice = name)}
        onRefresh={loadDevices}
      />
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
    {/snippet}
    {#snippet signalPath()}
      {#if status.track}
        <SignalPath track={status.track} device={playingDevice} />
      {/if}
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
  /* The current page carries the brand accent, as in the design guide. */
  nav [aria-current='page'],
  nav [aria-current='page']:hover:not(:disabled) {
    color: var(--accent-light);
    background: var(--accent-dark);
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
    overflow-y: auto;
  }
  /* Views flow top to bottom; one may fill the height (Artists). */
  main > :global(*),
  .keep > :global(*) {
    flex-shrink: 0;
  }
  /* The library stays mounted while hidden: it lays out as part of main. */
  .keep {
    display: contents;
  }
  .keep[hidden] {
    display: none;
  }
  /* Holds the toast over the view, whatever the view scrolls to. */
  .stage {
    flex: 1;
    min-width: 0;
    max-width: 1400px;
    display: flex;
    position: relative;
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

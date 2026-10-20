<script lang="ts">
  import { onMount } from 'svelte';
  import { BackendError, getAppInfo } from './lib/ipc';

  let status = $state('Connecting to Rust…');
  let connected = $state(false);
  let checking = $state(false);
  const laterViews = ['Albums', 'Artists', 'Playlists', 'Settings'];
  const albums = [
    {
      title: 'Still water',
      artist: 'Mira Sol',
      format: 'FLAC · 24 / 96',
      color: 'water',
    },
    {
      title: 'Night forms',
      artist: 'The Quiet Hours',
      format: 'DSD64 · DSF',
      color: 'night',
    },
    {
      title: 'Open spaces',
      artist: 'Aster Ensemble',
      format: 'WAV · 16 / 44.1',
      color: 'space',
    },
  ];

  async function checkBackend() {
    checking = true;
    connected = false;

    try {
      const info = await getAppInfo();

      status = `${info.name} ${info.version} · Rust connected · ${info.platform}`;
      connected = true;
    } catch (error) {
      status =
        error instanceof BackendError
          ? error.message
          : 'Unexpected error while contacting Rust.';
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    void checkBackend();
  });
</script>

<svelte:head><title>Lyra — Music player</title></svelte:head>

<div class="app">
  <div class="layout">
    <aside aria-label="Application">
      <h1><span aria-hidden="true">♮</span> Lyra</h1>
      <nav aria-label="Main navigation">
        <button type="button" aria-current="page">Library</button>
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
          <p class="eyebrow">YOUR LIBRARY</p>
          <h2>A little room for listening.</h2>
          <p class="muted">Your music, close at hand.</p>
        </div>
        <span class="badge">M0 · Preview</span>
      </header>
      <section class="backend" aria-label="Backend connection">
        <div>
          <h3>Desktop connection</h3>
          <p
            role="status"
            data-testid="backend-status"
            data-connected={connected}
          >
            {status}
          </p>
        </div>
        <button onclick={checkBackend} disabled={checking}
          >{checking ? 'Checking…' : 'Check backend'}</button
        >
      </section>
      <section aria-labelledby="albums-title">
        <div class="section-heading">
          <h3 id="albums-title">Sample library</h3>
          <span class="muted">3 mock albums</span>
        </div>
        <div class="albums">
          {#each albums as album (album.title)}
            <article>
              <div class="cover {album.color}" aria-hidden="true">
                <div></div>
                <span>Lyra Sessions</span>
              </div>
              <h4>{album.title}</h4>
              <p>{album.artist}</p>
              <small>{album.format}</small>
            </article>
          {/each}
        </div>
        <p class="note">
          Sample artwork and metadata. Library import and audio playback arrive
          in later milestones.
        </p>
      </section>
    </main>
  </div>
  <footer aria-label="Mock player">
    <div class="art" aria-hidden="true">♪</div>
    <div class="track">
      <strong>Nothing playing</strong><small>Audio engine not implemented</small
      >
    </div>
    <div class="controls">
      <button disabled aria-label="Previous track">⏮</button><button
        disabled
        aria-label="Play">▶</button
      ><button disabled aria-label="Next track">⏭</button>
    </div>
    <div class="output">
      <strong>No DAC selected</strong><small
        >Output inactive · Bit-perfect unverified</small
      >
    </div>
  </footer>
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
  .albums {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 24px;
  }
  .cover {
    aspect-ratio: 1;
    border-radius: 8px;
    position: relative;
    display: grid;
    place-items: center;
    overflow: hidden;
  }
  .water {
    background: linear-gradient(155deg, #8cab9c, #324b4d 60%, #172a36);
  }
  .night {
    background: linear-gradient(30deg, #1c2340, #71617a);
  }
  .space {
    background: linear-gradient(155deg, #d2b98d, #7d8770 55%, #364e44);
  }
  .cover div {
    width: 62%;
    height: 62%;
    border: 1px solid #ffffff50;
    border-radius: 50%;
    box-shadow:
      0 0 0 20px #ffffff09,
      0 0 0 40px #ffffff09;
  }
  .cover span {
    position: absolute;
    bottom: 17px;
    font-size: 9px;
    letter-spacing: 3px;
    text-transform: uppercase;
    color: #fff;
  }
  h4 {
    font-size: 16px;
    font-weight: 500;
    margin: 16px 0 5px;
  }
  article p {
    font-size: 13px;
    margin: 0 0 8px;
    color: #b1bbc4;
  }
  article small {
    font-size: 11px;
    color: #b4d9cb;
  }
  .note {
    color: #a2adb7;
    font-size: 12px;
    line-height: 1.6;
    margin-top: 24px;
  }
  .backend {
    border-block: 1px solid #29343d;
    padding-block: 16px;
  }
  .backend p {
    font-size: 12px;
    color: #b4d9cb;
    line-height: 1.5;
    margin-bottom: 0;
  }
  footer {
    min-height: 100px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 18px 26px;
    background: #1a232b;
    border-top: 1px solid #303c46;
  }
  .art {
    background: #31463f;
    color: #b4d9cb;
    border-radius: 8px;
    width: 54px;
    height: 54px;
    display: grid;
    place-items: center;
    font-size: 24px;
  }
  .track,
  .output {
    display: flex;
    flex-direction: column;
    gap: 7px;
    font-size: 13px;
  }
  .track small,
  .output small {
    color: #a2adb7;
    font-size: 11px;
  }
  .controls {
    flex: 1;
    display: flex;
    gap: 10px;
    justify-content: center;
  }
  .controls button {
    border: 0;
    background: transparent;
  }
  .output {
    text-align: right;
  }
  @media (max-width: 950px) {
    main {
      padding: 30px 25px;
    }
    aside {
      width: 170px;
      padding: 25px 15px;
    }
    .albums {
      gap: 16px;
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
    .output {
      display: none;
    }
    footer {
      gap: 10px;
      padding: 15px;
    }
    .art {
      display: none;
    }
    .controls {
      gap: 0;
    }
    .backend {
      align-items: flex-start;
    }
    .cover span {
      display: none;
    }
  }
</style>

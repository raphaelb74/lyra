<script lang="ts">
  import { attempt, type Backend, type Settings } from './ipc';
  import RadioGroup, { type Option } from './RadioGroup.svelte';

  interface Props {
    api: Backend;
    onError: (message: string) => void;
  }

  const { api, onError }: Props = $props();

  const transports: Option<Settings['dsdTransport']>[] = [
    {
      value: 'auto',
      label: 'Auto',
      hint: 'Native DSD when the DAC offers it, otherwise DoP.',
    },
    {
      value: 'native',
      label: 'Native only',
      hint: 'Refuse DSD on DACs without a native DSD mode.',
    },
    {
      value: 'dop',
      label: 'DoP only',
      hint: 'DSD in PCM frames, for DACs that decode DoP.',
    },
  ];
  const fallbacks: Option<Settings['dsdFallback']>[] = [
    {
      value: 'refuse',
      label: 'Refuse',
      hint: 'Say why the track cannot play. Nothing is converted.',
    },
    {
      value: 'convert',
      label: 'Convert to PCM',
      hint: 'Play it as 24-bit PCM at 88.2 or 44.1 kHz. Never bit-perfect, and always labelled so.',
    },
  ];
  const gains: Option<number>[] = [
    {
      value: 0,
      label: '0 dB',
      hint: 'Never clips: DSD peaks convert to about −3 dBFS.',
    },
    {
      value: 3,
      label: '+3 dB',
      hint: 'Closer to PCM loudness; the loudest masters may clip slightly.',
    },
    {
      value: 6,
      label: '+6 dB',
      hint: 'Matches the SACD reference level to PCM; loud masters clip.',
    },
  ];

  const delays: Option<number>[] = [
    { value: 0, label: 'Off', hint: 'Start at once.' },
    { value: 250, label: '0.25 s', hint: 'For DACs that switch quickly.' },
    {
      value: 500,
      label: '0.5 s',
      hint: 'Default: enough for the DACs tested so far.',
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

  $effect(() => {
    void load();
  });
</script>

<section aria-labelledby="settings-title">
  <div class="heading">
    <h3 id="settings-title">Settings</h3>
    {#if saved}
      <span class="saved" role="status"
        >Saved. Applies from the next track.</span
      >
    {/if}
  </div>

  {#if settings}
    <RadioGroup
      legend="DSD transport"
      name="dsd-transport"
      options={transports}
      value={settings.dsdTransport}
      onChoose={chooseTransport}
    />

    <RadioGroup
      legend="Pause before playback when the format changes"
      name="resync-delay"
      options={delays}
      value={settings.resyncDelayMs}
      onChoose={chooseDelay}
    >
      Many DACs need a moment to switch between sample rates, or between PCM and
      DSD; without it the first notes can be noise or missing. Lyra plays
      silence first, only when the format changes.
    </RadioGroup>

    <RadioGroup
      legend="DSD on devices without DSD support"
      name="dsd-fallback"
      options={fallbacks}
      value={settings.dsdFallback}
      onChoose={chooseFallback}
    >
      Built-in audio and most non-USB outputs take neither native DSD nor DoP.
      {#if settings.dsdTransport !== 'auto'}
        Conversion only applies with the Auto transport.
      {/if}
    </RadioGroup>

    {#if settings.dsdFallback === 'convert'}
      <RadioGroup
        legend="Conversion gain"
        name="conversion-gain"
        options={gains}
        value={settings.conversionGainDb}
        disabled={!converting}
        onChoose={chooseGain}
      />
    {/if}
  {:else}
    <p class="muted">Loading settings…</p>
  {/if}
</section>

<style>
  .heading {
    display: flex;
    align-items: baseline;
    gap: 16px;
    margin-bottom: 16px;
  }
  h3 {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
  }
  .saved {
    color: var(--accent);
    font-size: 13px;
  }
  .muted {
    color: var(--text-muted);
    font-size: 13px;
  }
</style>

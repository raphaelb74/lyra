<script lang="ts">
  import { attempt, type Backend, type Settings } from './ipc';

  interface Props {
    api: Backend;
    onError: (message: string) => void;
  }

  const { api, onError }: Props = $props();

  interface Option<T> {
    value: T;
    label: string;
    hint: string;
  }

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
    <fieldset>
      <legend>DSD transport</legend>
      {#each transports as option (option.value)}
        <label>
          <input
            type="radio"
            name="dsd-transport"
            value={option.value}
            checked={settings.dsdTransport === option.value}
            onchange={() => {
              chooseTransport(option.value);
            }}
          />
          <span>{option.label}<small>{option.hint}</small></span>
        </label>
      {/each}
    </fieldset>

    <fieldset>
      <legend>DSD on devices without DSD support</legend>
      <p class="muted">
        Built-in audio and most non-USB outputs take neither native DSD nor DoP.
        {#if settings.dsdTransport !== 'auto'}
          Conversion only applies with the Auto transport.
        {/if}
      </p>
      {#each fallbacks as option (option.value)}
        <label>
          <input
            type="radio"
            name="dsd-fallback"
            value={option.value}
            checked={settings.dsdFallback === option.value}
            onchange={() => {
              chooseFallback(option.value);
            }}
          />
          <span>{option.label}<small>{option.hint}</small></span>
        </label>
      {/each}
    </fieldset>

    {#if settings.dsdFallback === 'convert'}
      <fieldset disabled={!converting}>
        <legend>Conversion gain</legend>
        {#each gains as option (option.value)}
          <label>
            <input
              type="radio"
              name="conversion-gain"
              value={option.value}
              checked={settings.conversionGainDb === option.value}
              onchange={() => {
                chooseGain(option.value);
              }}
            />
            <span>{option.label}<small>{option.hint}</small></span>
          </label>
        {/each}
      </fieldset>
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
    color: #b4d9cb;
    font-size: 13px;
  }
  fieldset {
    border: 1px solid #29343d;
    border-radius: 8px;
    padding: 12px 16px 16px;
    margin: 0 0 16px;
    max-width: 640px;
    display: grid;
    gap: 6px;
  }
  fieldset:disabled {
    opacity: 0.5;
  }
  legend {
    padding: 0 6px;
    font-weight: 500;
  }
  label {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 6px 4px;
    border-radius: 6px;
    cursor: pointer;
  }
  label:hover {
    background: #182129;
  }
  input {
    margin-top: 3px;
    accent-color: #577369;
  }
  label span {
    display: grid;
    gap: 2px;
  }
  small,
  .muted {
    color: #a2adb7;
    font-size: 13px;
  }
  .muted {
    margin: 0 0 4px;
  }
</style>

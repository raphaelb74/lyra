<script lang="ts">
  interface Props {
    /** Accessible name, e.g. "Seek". */
    label: string;
    value: number;
    min?: number;
    max: number;
    step?: number;
    /** Spoken value, e.g. "1:23 of 4:56". */
    valueText: string;
    disabled?: boolean;
    title?: string;
    /** Every movement, while dragging too. */
    onInput?: (value: number) => void;
    /** Once, when the user lets go. */
    onChange?: (value: number) => void;
  }

  const {
    label,
    value,
    min = 0,
    max,
    step = 1,
    valueText,
    disabled = false,
    title,
    onInput,
    onChange,
  }: Props = $props();

  /** Held while dragging, so outside updates don't make the handle jump. */
  let held = $state<number | null>(null);

  const shown = $derived(held ?? value);
  const fill = $derived(
    max > min
      ? ((Math.min(max, Math.max(min, shown)) - min) / (max - min)) * 100
      : 0,
  );
</script>

<input
  class="slider"
  type="range"
  aria-label={label}
  aria-valuetext={valueText}
  {title}
  {min}
  {max}
  {step}
  value={shown}
  {disabled}
  style:--fill={`${String(fill)}%`}
  oninput={(event) => {
    held = event.currentTarget.valueAsNumber;
    onInput?.(held);
  }}
  onchange={(event) => {
    held = null;
    onChange?.(event.currentTarget.valueAsNumber);
  }}
/>

<style>
  /* A thin precise line: filled part in text colour, handle on demand. */
  .slider {
    --track: 2px;
    --thumb: 12px;

    appearance: none;
    /* Fills its row and shrinks with it, beside labels and icons. */
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
    height: var(--hit-row);
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  .slider:disabled {
    cursor: default;
    opacity: 0.4;
  }
  .slider:hover:not(:disabled),
  .slider:focus-visible,
  .slider:active {
    --track: 4px;
  }
  .slider:focus-visible {
    outline-offset: 0;
    border-radius: var(--radius-s);
  }

  .slider::-webkit-slider-runnable-track {
    height: var(--track);
    border-radius: var(--track);
    background: linear-gradient(
      to right,
      var(--text) var(--fill),
      var(--line-strong) var(--fill)
    );
    transition: height var(--dur-fast) var(--ease);
  }
  .slider:hover:not(:disabled)::-webkit-slider-runnable-track,
  .slider:active::-webkit-slider-runnable-track {
    background: linear-gradient(
      to right,
      var(--accent) var(--fill),
      var(--line-strong) var(--fill)
    );
  }
  .slider::-webkit-slider-thumb {
    appearance: none;
    width: var(--thumb);
    height: var(--thumb);
    margin-top: calc((var(--track) - var(--thumb)) / 2);
    border: 0;
    border-radius: var(--radius-round);
    background: var(--text);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .slider:hover:not(:disabled)::-webkit-slider-thumb,
  .slider:focus-visible::-webkit-slider-thumb,
  .slider:active::-webkit-slider-thumb {
    opacity: 1;
  }

  /* After a click the slider stays thin until hovered again. */
  :global(:root[data-input='pointer'])
    .slider:focus-visible:not(:hover, :active) {
    --track: 2px;
  }
  :global(:root[data-input='pointer'])
    .slider:focus-visible:not(:hover, :active)::-webkit-slider-thumb {
    opacity: 0;
  }

  /* Firefox, for the browser preview only. */
  .slider::-moz-range-track {
    height: var(--track);
    border-radius: var(--track);
    background: var(--line-strong);
  }
  .slider::-moz-range-progress {
    height: var(--track);
    border-radius: var(--track);
    background: var(--text);
  }
  .slider::-moz-range-thumb {
    width: var(--thumb);
    height: var(--thumb);
    border: 0;
    border-radius: var(--radius-round);
    background: var(--text);
    opacity: 0;
  }
  .slider:hover:not(:disabled)::-moz-range-thumb,
  .slider:focus-visible::-moz-range-thumb {
    opacity: 1;
  }
</style>

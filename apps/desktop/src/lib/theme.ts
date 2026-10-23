import type { Option } from './segmented';
import { readText, writeText } from './storage';

export type Theme = 'champagne' | 'arctic-blue';

export const THEMES: Option<Theme>[] = [
  { value: 'champagne', label: 'Champagne', hint: 'Warm champagne accents.' },
  { value: 'arctic-blue', label: 'Arctic Blue', hint: 'Cool blue accents.' },
];

const THEME_KEY = 'lyra.appearance.theme';

/** A viewer preference: keep it separate from the audio engine settings. */
export function readTheme(): Theme {
  return readText(THEME_KEY) === 'arctic-blue' ? 'arctic-blue' : 'champagne';
}

/** Shows `theme` and remembers it for the next start. */
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  writeText(THEME_KEY, theme);
}

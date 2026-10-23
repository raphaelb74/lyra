import type { Option } from './segmented';

export type Theme = 'champagne' | 'arctic-blue';

export const THEMES: Option<Theme>[] = [
  { value: 'champagne', label: 'Champagne', hint: 'Warm champagne accents.' },
  { value: 'arctic-blue', label: 'Arctic Blue', hint: 'Cool blue accents.' },
];

const THEME_KEY = 'lyra.appearance.theme';

/** A viewer preference: keep it separate from the audio engine settings. */
export function readTheme(): Theme {
  try {
    return localStorage.getItem(THEME_KEY) === 'arctic-blue'
      ? 'arctic-blue'
      : 'champagne';
  } catch {
    return 'champagne';
  }
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;

  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // The theme still works when the webview cannot persist preferences.
  }
}

import { describe, expect, it } from 'vitest';

import tokens from './tokens.css?raw';
import { THEMES, type Theme } from '../lib/theme';

const components = import.meta.glob<string>('../**/*.svelte', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** Resolve aliases and the active accent theme, then return linear sRGB. */
function colour(name: string, theme: Theme): number[] {
  const block = new RegExp(`\\[data-theme='${theme}'\\] \\{([^}]+)\\}`).exec(
    tokens,
  )?.[1];

  const pattern = new RegExp(`--${name}:\\s*([^;]+);`);

  const value = (pattern.exec(block ?? '') ??
    pattern.exec(tokens))?.[1]?.trim();

  if (value === undefined) throw new Error(`No colour token --${name}`);

  const alias = /^var\(--([\w-]+)\)$/.exec(value)?.[1];

  if (alias !== undefined) return colour(alias, theme);

  if (!/^#[0-9a-f]{6}$/.test(value)) {
    throw new Error(`Unsupported colour for --${name}: ${value}`);
  }

  return [1, 3, 5].map((start) => {
    const channel = parseInt(value.slice(start, start + 2), 16) / 255;

    return channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4;
  });
}

/** WCAG 2 relative luminance. */
function luminance(channels: number[]): number {
  const [r = 0, g = 0, b = 0] = channels;

  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(foreground: number[], background: number[]): number {
  const [light, dark] = [luminance(foreground), luminance(background)].sort(
    (a, b) => b - a,
  );

  return ((light ?? 0) + 0.05) / ((dark ?? 0) + 0.05);
}

function styles(source: string): string {
  return /<style>([\s\S]*?)<\/style>/.exec(source)?.[1] ?? '';
}

describe.each(THEMES)('$label contrast', ({ value: theme }) => {
  it.each([
    ['text', 'surface-selected'],
    ['text-secondary', 'surface-selected'],
    ['text-secondary', 'surface-hover'],
    ['text-on-emphasis', 'surface-selected-hover'],
    ['text-on-emphasis', 'accent-dark'],
    ['text-muted', 'bg'],
    ['text-muted', 'surface'],
    ['text-muted', 'surface-raised'],
    ['accent', 'surface-selected'],
    ['accent', 'accent-dark'],
    ['accent-light', 'accent-dark'],
    ['on-accent', 'accent'],
    ['on-accent', 'accent-light'],
    ['on-accent', 'accent-mid'],
    ['warning', 'surface-raised'],
    ['danger', 'surface-raised'],
  ])('%s on %s reads at 4.5:1 or more', (text, background) => {
    expect(
      contrast(colour(text, theme), colour(background, theme)),
    ).toBeGreaterThanOrEqual(4.5);
  });
});

describe('design tokens', () => {
  it('are the only colours, font sizes and durations in components', () => {
    const rules: [string, RegExp][] = [
      ['raw colour', /#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab)\(/i],
      ['raw font size', /font-size:(?!\s*var\()/],
      ['raw duration', /\b\d+(?:\.\d+)?m?s\b/],
    ];

    const found: string[] = [];

    for (const [path, source] of Object.entries(components)) {
      const css = styles(source);

      for (const [rule, pattern] of rules) {
        if (pattern.test(css)) found.push(`${path}: ${rule}`);
      }
    }

    expect(found).toEqual([]);
  });
});

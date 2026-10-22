import { describe, expect, it } from 'vitest';

import tokens from './tokens.css?raw';

const components = import.meta.glob<string>('../**/*.svelte', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** `#rrggbb` value of a token in tokens.css. */
function colour(name: string): string {
  const found = new RegExp(`--${name}:\\s*(#[0-9a-f]{6});`).exec(tokens);

  if (!found?.[1]) throw new Error(`No colour token --${name}`);

  return found[1];
}

/** WCAG 2 relative luminance. */
function luminance(hex: string): number {
  const channels = [1, 3, 5].map((start) => {
    const value = parseInt(hex.slice(start, start + 2), 16) / 255;

    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });

  const [r = 0, g = 0, b = 0] = channels;

  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(foreground: string, background: string): number {
  const [light, dark] = [luminance(foreground), luminance(background)].sort(
    (a, b) => b - a,
  );

  return ((light ?? 0) + 0.05) / ((dark ?? 0) + 0.05);
}

function styles(source: string): string {
  return /<style>([\s\S]*?)<\/style>/.exec(source)?.[1] ?? '';
}

describe('design tokens', () => {
  it.each([
    ['text', 'surface-selected'],
    ['text-secondary', 'surface-selected'],
    ['text-muted', 'bg'],
    ['text-muted', 'surface'],
    ['text-muted', 'surface-raised'],
    ['accent', 'surface-selected'],
    ['warning', 'surface-raised'],
    ['danger', 'surface-raised'],
  ])('%s on %s reads at 4.5:1 or more', (text, background) => {
    expect(contrast(colour(text), colour(background))).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  it('are the only colours, font sizes and durations in components', () => {
    const rules: [string, RegExp][] = [
      ['raw colour', /#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|oklch)\(/i],
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

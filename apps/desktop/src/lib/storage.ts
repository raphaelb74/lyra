/**
 * Per-viewer preferences kept in localStorage (e.g. the queue pane open, the
 * accent theme). Storage can be missing or refuse writes; the app then just
 * forgets the choice.
 */

export function readText(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeText(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Not remembered this time.
  }
}

export function readFlag(key: string): boolean {
  return readText(key) === 'true';
}

export function writeFlag(key: string, value: boolean) {
  writeText(key, String(value));
}

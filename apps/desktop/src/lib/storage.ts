/**
 * Per-viewer on/off preferences kept in localStorage (e.g. the queue pane
 * open). Storage can be missing or refuse writes; the app then just
 * forgets the choice.
 */

export function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === 'true';
  } catch {
    return false;
  }
}

export function writeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // Not remembered this time.
  }
}

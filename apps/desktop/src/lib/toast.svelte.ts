/**
 * One short notice at a time, drawn by Toast.svelte above the player bar,
 * e.g. "Saved" with Undo. It leaves on its own, on a new view, or on ×.
 */

export interface ToastMessage {
  text: string;
  /** A quieter second part, e.g. "Files untouched". */
  detail?: string;
  action?: { label: string; run: () => void };
}

/** Long enough to reach Undo; paused while the pointer or focus is on it. */
const TOAST_MS = 8000;

export const toast = $state<{ current: ToastMessage | null }>({
  current: null,
});

let timer: ReturnType<typeof setTimeout> | undefined;

export function showToast(message: ToastMessage) {
  toast.current = message;
  resumeToast();
}

export function dismissToast() {
  holdToast();
  toast.current = null;
}

export function holdToast() {
  clearTimeout(timer);
}

export function resumeToast() {
  clearTimeout(timer);
  timer = setTimeout(dismissToast, TOAST_MS);
}

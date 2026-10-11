/**
 * Display-aware root scale (docs/05 §7.1.0). Everything in the app is in rem, so setting the root
 * size scales the whole UI.
 *
 * The scale follows the **display** (`screen.width`, in device-independent pixels), not the window:
 * page zoom does not change `screen.width`, so Cmd/Ctrl +/- applies fully on top, and a narrow
 * window on a big monitor keeps the big-monitor scale, unless the window is short (< 860px tall),
 * where the base size keeps dialogs and tables fitting. 16px up to 1440-wide displays, 17px at 1728,
 * about 17.7px at 1920, and 20px on 2560+ (4K at 150%, ultrawides). The result is a multiplier on
 * 100%, so the user's own browser font size still applies (WCAG 1.4.4).
 */
export function rootScale(screenWidth: number, windowHeight = Infinity): number {
  // A short window (a laptop-height browser on a big monitor, or a docked one) keeps the base size so
  // dialogs and tables still fit.
  if (windowHeight < SHORT_WINDOW) return 1;
  const scale = 1 + Math.max(0, screenWidth - 1440) / 1120 / 4;
  return Math.min(1.25, Math.max(1, scale));
}

/** Windows shorter than this (CSS px) stay at the base size. */
export const SHORT_WINDOW = 860;

export function applyRootScale(): void {
  const set = () => document.documentElement.style.setProperty('--root-scale', rootScale(window.screen.width, window.innerHeight).toFixed(4));
  set();
  // Moving the window to another monitor changes screen.width; a resize event follows.
  window.addEventListener('resize', set, { passive: true });
}

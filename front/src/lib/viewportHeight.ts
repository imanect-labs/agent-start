/**
 * Keeps `--app-h` honest about how much room the page actually has.
 *
 * Default is `100svh` (the smallest dynamic viewport) — always fits
 * under iOS Safari's overlay URL bar, at the cost of some wasted
 * margin when the toolbar is hidden. When `visualViewport.height` is
 * available *and* smaller than what svh implies (e.g. while the
 * on-screen keyboard is up), we override with the tighter value so
 * sheets / terminal / etc. don't disappear under the keyboard.
 *
 * We deliberately do NOT pick the larger of the two: extending past
 * `svh` is what makes the bottom row hide behind chrome again.
 *
 * The layout viewport is locked (`body { position: fixed; inset: 0 }` in
 * globals.css), so iOS can't scroll the *page* to lift a focused field above
 * the keyboard — which is what used to shove the header off-screen and made
 * an earlier version fight back with `window.scrollTo(0, 0)` on every scroll
 * event. That tug-of-war against iOS's own focus-scroll was the source of the
 * visible up/down jitter in the PWA. With the viewport locked we only resize.
 */
export function installAppHeightVar() {
  if (typeof window === "undefined") return;
  const root = document.documentElement;
  const vv = window.visualViewport;
  if (!vv) return;

  let raf = 0;
  let current = "";

  const apply = () => {
    raf = 0;
    // Compare against window.innerHeight as a proxy for "no chrome overlay" —
    // when innerHeight > visualViewport.height the OSK (or some other
    // temporary overlay) is up and we should shrink; otherwise trust CSS svh.
    const next = vv.height < window.innerHeight - 1 ? `${Math.round(vv.height)}px` : "";
    // Skip redundant writes: `scroll` fires continuously without the height
    // changing, and re-writing the var thrashes layout for no reason.
    if (next === current) return;
    current = next;
    if (next) root.style.setProperty("--app-h", next);
    else root.style.removeProperty("--app-h");
  };

  // Coalesce bursts of resize/scroll events into a single per-frame update.
  const schedule = () => {
    if (raf) return;
    raf = requestAnimationFrame(apply);
  };

  apply();
  vv.addEventListener("resize", schedule);
  vv.addEventListener("scroll", schedule);
  window.addEventListener("orientationchange", schedule);
}

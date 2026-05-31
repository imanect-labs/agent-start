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
 */
export function installAppHeightVar() {
  if (typeof window === "undefined") return;
  const root = document.documentElement;
  const vv = window.visualViewport;

  const apply = () => {
    if (!vv) return;
    // Compare against window.innerHeight as a proxy for "no chrome
    // overlay" — when innerHeight > visualViewport.height the OSK
    // (or some other temporary overlay) is up and we should shrink.
    if (vv.height < window.innerHeight - 1) {
      root.style.setProperty("--app-h", `${Math.round(vv.height)}px`);
    } else {
      // Toolbar may be in flux; trust CSS svh.
      root.style.removeProperty("--app-h");
    }

    // When a field near the bottom (e.g. the chat composer) is focused,
    // iOS scrolls the *layout* viewport up to lift it above the on-screen
    // keyboard. The app shell is `overflow:hidden`, so that scroll offset
    // can't rubber-band back — it sticks, shoving the header off the top
    // for good. The `--app-h` shrink above already keeps the composer
    // visible, so pin the layout viewport back to the origin.
    if (window.scrollY !== 0 || window.scrollX !== 0) {
      window.scrollTo(0, 0);
    }
  };

  apply();
  vv?.addEventListener("resize", apply);
  vv?.addEventListener("scroll", apply);
  window.addEventListener("orientationchange", apply);
  // iOS fires the focus-driven auto-scroll without a visualViewport event
  // in some standalone-PWA cases; catch it directly too.
  window.addEventListener("scroll", apply, { passive: true });
}

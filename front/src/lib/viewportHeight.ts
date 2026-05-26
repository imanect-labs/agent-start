/**
 * Drives the `--app-h` CSS custom property from `visualViewport.height`.
 *
 * `100dvh` is supposed to track the dynamic viewport but iOS Safari leaves
 * the bottom URL bar overlapping content until the user scrolls — so a
 * 100dvh layout still gets its bottom row hidden behind the chrome.
 * `visualViewport.height` reflects what is actually visible right now.
 *
 * Call once at app startup. The CSS variable is set on <html> so any
 * descendant can read it; we fall back to 100dvh in CSS when no JS has
 * run yet (e.g. before hydration).
 */
export function installAppHeightVar() {
  if (typeof window === "undefined") return;
  const root = document.documentElement;
  const vv = window.visualViewport;

  const apply = () => {
    const h = vv?.height ?? window.innerHeight;
    root.style.setProperty("--app-h", `${Math.round(h)}px`);
  };

  apply();
  vv?.addEventListener("resize", apply);
  vv?.addEventListener("scroll", apply);
  window.addEventListener("orientationchange", apply);
}

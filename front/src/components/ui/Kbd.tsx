import { ReactNode } from "react";

/**
 * Keyboard-shortcut chip (e.g. ⌘K, Esc). A small, low-contrast monospace key
 * used in menus, tooltips, and hints — a Linear-style affordance.
 */
export function Kbd({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={[
        "inline-flex items-center justify-center min-w-[1.25rem] h-[1.125rem] px-1",
        "rounded text-2xs font-mono font-medium leading-none",
        "bg-surface-muted text-fg-subtle border border-line shadow-elevate",
        className,
      ].join(" ")}
    >
      {children}
    </kbd>
  );
}

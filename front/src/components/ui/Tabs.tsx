import { ReactNode, useRef } from "react";
import { IconX } from "../icons";

export type TabItem = {
  id: string;
  label: ReactNode;
  icon?: ReactNode;
  closable?: boolean;
  closeLabel?: string;
};

/**
 * Horizontal tab strip with tablist semantics, roving arrow-key navigation,
 * an indigo active indicator, optional per-tab close, and overflow scrolling
 * with edge fades. The panels themselves are rendered by the caller.
 */
export function Tabs({
  items,
  activeId,
  onActivate,
  onClose,
  className = "",
  "aria-label": ariaLabel,
  trailing,
}: {
  items: TabItem[];
  activeId: string;
  onActivate: (id: string) => void;
  onClose?: (id: string) => void;
  className?: string;
  "aria-label"?: string;
  /** Rendered after the tabs (e.g. a "+" new-tab button). */
  trailing?: ReactNode;
}) {
  const stripRef = useRef<HTMLDivElement>(null);

  const focusTab = (id: string) => {
    stripRef.current?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(id)}"]`)?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const idx = items.findIndex((t) => t.id === activeId);
    if (idx < 0) return;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      const next = items[(idx + 1) % items.length];
      onActivate(next.id);
      focusTab(next.id);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      const prev = items[(idx - 1 + items.length) % items.length];
      onActivate(prev.id);
      focusTab(prev.id);
    } else if (e.key === "Home") {
      e.preventDefault();
      onActivate(items[0].id);
      focusTab(items[0].id);
    } else if (e.key === "End") {
      e.preventDefault();
      const last = items[items.length - 1];
      onActivate(last.id);
      focusTab(last.id);
    }
  };

  return (
    <div className={["flex items-stretch min-w-0", className].join(" ")}>
      <div
        ref={stripRef}
        role="tablist"
        aria-label={ariaLabel}
        onKeyDown={onKeyDown}
        className="flex items-stretch min-w-0 overflow-x-auto scroll-thin [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {items.map((t) => {
          const active = t.id === activeId;
          return (
            <div
              key={t.id}
              role="tab"
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              data-tab-id={t.id}
              onClick={() => onActivate(t.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onActivate(t.id);
                }
              }}
              className={[
                "group relative inline-flex items-center gap-1.5 px-3 h-9 shrink-0 cursor-pointer select-none",
                "text-xs font-medium whitespace-nowrap outline-none",
                "transition-colors focus-visible:bg-surface-muted",
                active
                  ? "text-fg bg-surface"
                  : "text-fg-subtle hover:text-fg hover:bg-surface-muted/60",
              ].join(" ")}
            >
              {t.icon && <span className="shrink-0 [&_svg]:w-3.5 [&_svg]:h-3.5">{t.icon}</span>}
              <span className="truncate max-w-[14rem]">{t.label}</span>
              {t.closable && onClose && (
                <button
                  type="button"
                  aria-label={t.closeLabel ?? "閉じる"}
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(t.id);
                  }}
                  className="ml-0.5 inline-flex items-center justify-center w-4 h-4 rounded-sm text-fg-faint opacity-0 group-hover:opacity-100 focus:opacity-100 hover:bg-surface-sunken hover:text-fg transition-opacity"
                >
                  <IconX className="w-3 h-3" />
                </button>
              )}
              {/* Active indicator */}
              <span
                aria-hidden
                className={[
                  "absolute left-0 right-0 bottom-0 h-0.5 transition-colors",
                  active ? "bg-accent" : "bg-transparent",
                ].join(" ")}
              />
            </div>
          );
        })}
      </div>
      {trailing && <div className="flex items-center shrink-0">{trailing}</div>}
    </div>
  );
}

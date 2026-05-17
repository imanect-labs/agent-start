"use client";

import { ReactNode, useEffect } from "react";

type Props = {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  maxWidth?: "md" | "lg" | "xl" | "2xl" | "3xl";
  /** when true, sheet fills (almost) the full viewport height */
  tall?: boolean;
};

const MAX_W: Record<NonNullable<Props["maxWidth"]>, string> = {
  md: "sm:max-w-md",
  lg: "sm:max-w-lg",
  xl: "sm:max-w-xl",
  "2xl": "sm:max-w-2xl",
  "3xl": "sm:max-w-3xl",
};

export function Sheet({
  open,
  onClose,
  children,
  maxWidth = "md",
  tall = false,
}: Props) {
  useEffect(() => {
    if (!open) return;
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.body.style.overflow = prev;
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-end sm:items-center justify-center"
      role="dialog"
      aria-modal="true"
    >
      <div
        className="absolute inset-0 bg-zinc-950/30 backdrop-blur-[2px]"
        onClick={onClose}
      />
      <div
        className={[
          "relative bg-white text-zinc-900 w-full",
          MAX_W[maxWidth],
          "rounded-t-2xl sm:rounded-xl",
          "border-t border-zinc-200 sm:border",
          "shadow-[0_-12px_40px_-12px_rgba(0,0,0,0.18)] sm:shadow-[0_24px_60px_-12px_rgba(0,0,0,0.25)]",
          "flex flex-col overflow-hidden",
          tall ? "h-[95dvh] sm:h-[92dvh]" : "max-h-[90vh]",
        ].join(" ")}
      >
        {children}
      </div>
    </div>
  );
}

export function SheetHeader({
  title,
  subtitle,
  onClose,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  onClose: () => void;
}) {
  return (
    <div className="flex items-start justify-between gap-2 px-5 pt-5 pb-4 border-b border-zinc-100">
      <div className="flex-1 min-w-0">
        <div className="text-base font-semibold text-zinc-900 truncate tracking-tight">
          {title}
        </div>
        {subtitle && (
          <div className="text-xs text-zinc-500 truncate mt-0.5">{subtitle}</div>
        )}
      </div>
      <button
        type="button"
        onClick={onClose}
        className="shrink-0 -mt-1 -mr-2 w-9 h-9 inline-flex items-center justify-center rounded-md text-zinc-400 hover:text-zinc-900 hover:bg-zinc-100 transition-colors"
        aria-label="閉じる"
      >
        <svg viewBox="0 0 20 20" className="w-4 h-4" fill="currentColor">
          <path d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z" />
        </svg>
      </button>
    </div>
  );
}

export function SheetBody({
  children,
  noScroll,
}: {
  children: ReactNode;
  /** disable internal scroll; child is expected to manage its own height (e.g. terminal) */
  noScroll?: boolean;
}) {
  return (
    <div
      className={
        noScroll
          ? "flex-1 overflow-hidden px-5 py-4 min-h-0 flex flex-col"
          : "flex-1 overflow-y-auto px-5 py-4 space-y-5"
      }
    >
      {children}
    </div>
  );
}

export function SheetFooter({ children }: { children: ReactNode }) {
  return (
    <div className="flex gap-2 px-5 py-3.5 border-t border-zinc-100 safe-bottom">
      {children}
    </div>
  );
}

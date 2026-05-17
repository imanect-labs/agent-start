"use client";

import { ReactNode, useEffect } from "react";

type Props = {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  // visual size on desktop. mobile is always full-width
  maxWidth?: "md" | "lg" | "xl" | "2xl" | "3xl";
};

const MAX_W: Record<NonNullable<Props["maxWidth"]>, string> = {
  md: "sm:max-w-md",
  lg: "sm:max-w-lg",
  xl: "sm:max-w-xl",
  "2xl": "sm:max-w-2xl",
  "3xl": "sm:max-w-3xl",
};

export function Sheet({ open, onClose, children, maxWidth = "md" }: Props) {
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
      className="fixed inset-0 z-[100] flex items-end sm:items-center justify-center safe-bottom"
      role="dialog"
      aria-modal="true"
    >
      <div
        className="absolute inset-0 bg-black/40"
        onClick={onClose}
      />
      <div
        className={`relative bg-white text-zinc-900 w-full ${MAX_W[maxWidth]} rounded-t-2xl sm:rounded-2xl shadow-2xl flex flex-col max-h-[90vh] overflow-hidden`}
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
    <div className="flex items-start justify-between gap-2 px-5 pt-5 pb-3 border-b border-zinc-100">
      <div className="flex-1 min-w-0">
        <div className="text-lg font-bold text-zinc-900 truncate">{title}</div>
        {subtitle && (
          <div className="text-xs text-zinc-500 truncate mt-0.5">{subtitle}</div>
        )}
      </div>
      <button
        type="button"
        onClick={onClose}
        className="shrink-0 -mt-1 -mr-2 w-10 h-10 inline-flex items-center justify-center text-zinc-500 hover:text-zinc-900"
        aria-label="閉じる"
      >
        ✕
      </button>
    </div>
  );
}

export function SheetBody({ children }: { children: ReactNode }) {
  return (
    <div className="flex-1 overflow-y-auto px-5 py-4 space-y-5">{children}</div>
  );
}

export function SheetFooter({ children }: { children: ReactNode }) {
  return (
    <div className="flex gap-2 px-5 py-4 border-t border-zinc-100 safe-bottom">
      {children}
    </div>
  );
}

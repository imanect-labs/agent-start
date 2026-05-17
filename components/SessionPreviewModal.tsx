"use client";

import { useEffect } from "react";
import { Terminal } from "./Terminal";

type Props = {
  sessionName: string | null;
  isOpen: boolean;
  onClose: () => void;
};

export function SessionPreviewModal({ sessionName, isOpen, onClose }: Props) {
  useEffect(() => {
    if (!isOpen) return;
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
  }, [isOpen, onClose]);

  if (!isOpen || !sessionName) return null;

  return (
    <div
      className="fixed inset-0 z-[100] bg-zinc-50 flex flex-col"
      role="dialog"
      aria-modal="true"
    >
      <div className="safe-top shrink-0 bg-white border-b border-zinc-200">
        <div className="px-3 py-2 flex items-center gap-2">
          <button
            type="button"
            onClick={onClose}
            className="-ml-1 w-10 h-10 inline-flex items-center justify-center rounded-md text-zinc-600 hover:text-zinc-900 hover:bg-zinc-100 active:bg-zinc-200 transition-colors"
            aria-label="戻る"
          >
            <svg
              viewBox="0 0 20 20"
              className="w-5 h-5"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M12.5 4 6.5 10l6 6" />
            </svg>
          </button>
          <div className="flex-1 min-w-0">
            <div className="text-[10px] uppercase tracking-wider text-zinc-500 font-medium leading-none">
              terminal
            </div>
            <div className="font-mono text-sm break-all truncate text-zinc-900 mt-0.5">
              {sessionName}
            </div>
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0 px-3 pt-3 pb-3 safe-bottom">
        <Terminal sessionName={sessionName} />
      </div>
    </div>
  );
}

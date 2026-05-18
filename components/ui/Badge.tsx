"use client";

import { ReactNode } from "react";

type Tone = "neutral" | "blue" | "emerald" | "amber" | "red" | "violet";

const TONE: Record<Tone, string> = {
  neutral: "bg-surface-muted text-fg-muted border-line",
  blue: "bg-blue-500/10 text-blue-600 border-blue-500/20 dark:text-blue-300",
  emerald:
    "bg-emerald-500/10 text-emerald-700 border-emerald-500/20 dark:text-emerald-300",
  amber:
    "bg-amber-500/10 text-amber-700 border-amber-500/30 dark:text-amber-300",
  red: "bg-red-500/10 text-red-600 border-red-500/20 dark:text-red-300",
  violet:
    "bg-violet-500/10 text-violet-700 border-violet-500/20 dark:text-violet-300",
};

export function Badge({
  tone = "neutral",
  children,
  className = "",
}: {
  tone?: Tone;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={[
        "inline-flex items-center gap-1",
        "text-[10px] font-medium leading-none",
        "px-1.5 py-1 rounded border",
        TONE[tone],
        className,
      ].join(" ")}
    >
      {children}
    </span>
  );
}

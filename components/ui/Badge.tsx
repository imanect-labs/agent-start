"use client";

import { ReactNode } from "react";

type Tone = "neutral" | "blue" | "emerald" | "amber" | "red" | "violet";

const TONE: Record<Tone, string> = {
  neutral: "bg-zinc-100 text-zinc-700 border-zinc-200",
  blue: "bg-blue-50 text-blue-700 border-blue-200",
  emerald: "bg-emerald-50 text-emerald-700 border-emerald-200",
  amber: "bg-amber-50 text-amber-800 border-amber-200",
  red: "bg-red-50 text-red-700 border-red-200",
  violet: "bg-violet-50 text-violet-700 border-violet-200",
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

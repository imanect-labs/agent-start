import { ReactNode } from "react";

type Tone = "neutral" | "accent" | "indigo" | "blue" | "emerald" | "amber" | "red" | "violet";
type Size = "xs" | "sm";

const TONE: Record<Tone, string> = {
  neutral: "bg-surface-muted text-fg-muted border-line",
  accent: "bg-accent-soft text-accent-subtle border-accent/20",
  indigo: "bg-indigo-500/10 text-indigo-600 border-indigo-500/20 dark:text-indigo-300",
  blue: "bg-blue-500/10 text-blue-600 border-blue-500/20 dark:text-blue-300",
  emerald: "bg-emerald-500/10 text-emerald-700 border-emerald-500/20 dark:text-emerald-300",
  amber: "bg-amber-500/10 text-amber-700 border-amber-500/30 dark:text-amber-300",
  red: "bg-red-500/10 text-red-600 border-red-500/20 dark:text-red-300",
  violet: "bg-violet-500/10 text-violet-700 border-violet-500/20 dark:text-violet-300",
};

// Status-dot colors mirror the tone but stay legible as a tiny solid dot.
const DOT: Record<Tone, string> = {
  neutral: "bg-fg-faint",
  accent: "bg-accent",
  indigo: "bg-indigo-500",
  blue: "bg-blue-500",
  emerald: "bg-emerald-500",
  amber: "bg-amber-500",
  red: "bg-red-500",
  violet: "bg-violet-500",
};

const SIZE: Record<Size, string> = {
  xs: "text-2xs px-1.5 py-0.5 gap-1",
  sm: "text-xs px-2 py-0.5 gap-1.5",
};

export function Badge({
  tone = "neutral",
  size = "xs",
  dot = false,
  children,
  className = "",
}: {
  tone?: Tone;
  size?: Size;
  /** Render a leading status dot in the tone color. */
  dot?: boolean;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={[
        "inline-flex items-center font-medium leading-none rounded-sm border",
        SIZE[size],
        TONE[tone],
        className,
      ].join(" ")}
    >
      {dot && <span className={["inline-block w-1.5 h-1.5 rounded-full", DOT[tone]].join(" ")} />}
      {children}
    </span>
  );
}

import { ReactNode } from "react";

// Tones derive entirely from design tokens (no saturated Tailwind hues).
// Decorative aliases (indigo/blue/violet/emerald/amber/red) collapse onto the
// semantic palette so callers keep working while the look stays disciplined.
type Tone =
  | "neutral"
  | "accent"
  | "success"
  | "warn"
  | "danger"
  | "indigo"
  | "blue"
  | "violet"
  | "emerald"
  | "amber"
  | "red";
type Size = "xs" | "sm";

const TONE: Record<Tone, string> = {
  neutral: "bg-surface-muted text-fg-muted border-line",
  accent: "bg-accent-soft text-accent-subtle border-accent/25",
  success: "bg-success-soft text-success border-success/25",
  warn: "bg-warn-soft text-warn border-warn/25",
  danger: "bg-danger-soft text-danger border-danger/25",
  // aliases → semantic
  indigo: "bg-accent-soft text-accent-subtle border-accent/25",
  blue: "bg-accent-soft text-accent-subtle border-accent/25",
  violet: "bg-accent-soft text-accent-subtle border-accent/25",
  emerald: "bg-success-soft text-success border-success/25",
  amber: "bg-warn-soft text-warn border-warn/25",
  red: "bg-danger-soft text-danger border-danger/25",
};

const DOT: Record<Tone, string> = {
  neutral: "bg-fg-faint",
  accent: "bg-accent",
  success: "bg-success",
  warn: "bg-warn",
  danger: "bg-danger",
  indigo: "bg-accent",
  blue: "bg-accent",
  violet: "bg-accent",
  emerald: "bg-success",
  amber: "bg-warn",
  red: "bg-danger",
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

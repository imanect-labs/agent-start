import { ReactNode } from "react";

export type SegmentOption<T extends string> = {
  value: T;
  label: ReactNode;
  icon?: ReactNode;
  "aria-label"?: string;
};

type Size = "sm" | "md";

const SIZE: Record<Size, string> = {
  sm: "h-8 text-xs",
  md: "h-9 text-sm",
};

/**
 * A compact group of mutually-exclusive options rendered as a pill toggle.
 * Use for view-mode switches (split/unified, list/tree, theme picker).
 */
export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  size = "md",
  className = "",
  "aria-label": ariaLabel,
}: {
  value: T;
  onChange: (value: T) => void;
  options: SegmentOption<T>[];
  size?: Size;
  className?: string;
  "aria-label"?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={[
        "inline-flex items-center gap-0.5 p-0.5 rounded bg-surface-muted border border-line",
        className,
      ].join(" ")}
    >
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={opt["aria-label"]}
            onClick={() => onChange(opt.value)}
            className={[
              "inline-flex items-center justify-center gap-1.5 px-2.5 rounded-sm font-medium whitespace-nowrap",
              "transition-colors duration-150 outline-none",
              "focus-visible:ring-2 focus-visible:ring-ring/30",
              SIZE[size],
              active ? "bg-surface text-fg shadow-xs" : "text-fg-subtle hover:text-fg",
            ].join(" ")}
          >
            {opt.icon}
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

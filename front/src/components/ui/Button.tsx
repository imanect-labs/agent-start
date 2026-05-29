import { ButtonHTMLAttributes, forwardRef, ReactNode } from "react";

// `accent` is the brand/primary intent (indigo after the Phase 2 flip).
// `neutral` keeps the old solid near-black/near-white control look, decoupled
// from the accent so it survives the flip. `primary` is an alias of `accent`.
type Variant =
  | "accent"
  | "primary"
  | "neutral"
  | "secondary"
  | "ghost"
  | "danger"
  | "dangerOutline";
type Size = "sm" | "md" | "lg" | "icon";

// Primary/accent get a faint top inner-highlight (shadow-elevate) for the
// crafted, slightly-raised look; the focus ring is the purple accent.
const VARIANT: Record<Variant, string> = {
  accent: "bg-accent text-accent-fg border border-accent/60 hover:bg-accent-hover shadow-elevate",
  primary: "bg-accent text-accent-fg border border-accent/60 hover:bg-accent-hover shadow-elevate",
  neutral:
    "bg-neutral-strong text-neutral-strong-fg border border-neutral-strong shadow-elevate hover:opacity-90",
  secondary:
    "bg-surface text-fg border border-line hover:bg-surface-muted hover:border-line-strong",
  ghost:
    "bg-transparent text-fg-muted border border-transparent hover:bg-surface-muted hover:text-fg",
  danger: "bg-danger text-danger-fg border border-danger/60 shadow-elevate hover:opacity-90",
  dangerOutline:
    "bg-transparent text-danger border border-danger/30 hover:bg-danger/10 hover:border-danger/50",
};

const SIZE: Record<Size, string> = {
  sm: "h-7 px-2.5 text-xs rounded",
  md: "h-8 px-3 text-sm rounded",
  lg: "h-9 px-4 text-sm rounded",
  icon: "h-8 w-8 text-sm rounded",
};

const GAP: Record<Size, string> = {
  sm: "gap-1.5",
  md: "gap-2",
  lg: "gap-2",
  icon: "gap-0",
};

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  leftIcon?: ReactNode;
  rightIcon?: ReactNode;
};

export const Button = forwardRef<HTMLButtonElement, Props>(function Button(
  {
    variant = "secondary",
    size = "md",
    loading,
    leftIcon,
    rightIcon,
    disabled,
    className = "",
    children,
    type = "button",
    ...rest
  },
  ref,
) {
  const isDisabled = disabled || loading;
  return (
    <button
      ref={ref}
      type={type}
      disabled={isDisabled}
      className={[
        "relative inline-flex items-center justify-center whitespace-nowrap font-medium",
        "transition-[background-color,border-color,color,box-shadow,transform,opacity] duration-[130ms] ease-out",
        "active:scale-[0.98] motion-reduce:active:scale-100 [@media(pointer:coarse)]:active:scale-100",
        "outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-offset-2 focus-visible:ring-offset-app",
        "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none disabled:shadow-none",
        "select-none",
        VARIANT[variant],
        SIZE[size],
        className,
      ].join(" ")}
      {...rest}
    >
      {loading && (
        <span className="absolute inset-0 inline-flex items-center justify-center">
          <span
            aria-label="読み込み中"
            role="status"
            className="inline-block h-3.5 w-3.5 rounded-full border-2 border-current border-r-transparent animate-spin"
          />
        </span>
      )}
      <span
        className={["inline-flex items-center", GAP[size], loading ? "invisible" : ""].join(" ")}
      >
        {leftIcon}
        {children}
        {rightIcon}
      </span>
    </button>
  );
});

import { ButtonHTMLAttributes, forwardRef, ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "dangerOutline";
type Size = "sm" | "md" | "lg";

const VARIANT: Record<Variant, string> = {
  primary:
    "bg-accent text-accent-fg border border-accent hover:bg-accent-hover hover:border-accent-hover",
  secondary:
    "bg-surface text-fg border border-line hover:bg-surface-muted hover:border-line-strong",
  ghost:
    "bg-transparent text-fg-muted border border-transparent hover:bg-surface-muted hover:text-fg",
  danger: "bg-danger text-danger-fg border border-danger hover:opacity-90",
  dangerOutline:
    "bg-surface text-danger border border-danger/30 hover:bg-danger/5 hover:border-danger/50",
};

const SIZE: Record<Size, string> = {
  sm: "h-8 px-3 text-xs rounded-md",
  md: "h-10 px-3.5 text-sm rounded-md",
  lg: "h-11 px-4 text-sm rounded-md",
};

const GAP: Record<Size, string> = {
  sm: "gap-1.5",
  md: "gap-2",
  lg: "gap-2",
};

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  leftIcon?: ReactNode;
};

export const Button = forwardRef<HTMLButtonElement, Props>(function Button(
  {
    variant = "secondary",
    size = "md",
    loading,
    leftIcon,
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
        "transition-colors duration-150",
        "outline-none focus-visible:ring-2 focus-visible:ring-ring/20 focus-visible:ring-offset-1 focus-visible:ring-offset-surface",
        "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none",
        "select-none",
        VARIANT[variant],
        SIZE[size],
        className,
      ].join(" ")}
      {...rest}
    >
      {loading && (
        <span aria-hidden className="absolute inset-0 inline-flex items-center justify-center">
          <span className="inline-block h-3.5 w-3.5 rounded-full border-2 border-current border-r-transparent animate-spin" />
        </span>
      )}
      <span
        className={["inline-flex items-center", GAP[size], loading ? "invisible" : ""].join(" ")}
      >
        {leftIcon}
        {children}
      </span>
    </button>
  );
});

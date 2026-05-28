import { ButtonHTMLAttributes, forwardRef, ReactNode } from "react";

type Variant = "ghost" | "subtle" | "danger";
type Size = "sm" | "md";

const VARIANT: Record<Variant, string> = {
  ghost: "text-fg-muted hover:bg-surface-muted hover:text-fg",
  subtle: "bg-surface-muted text-fg-muted hover:bg-surface-sunken hover:text-fg border border-line",
  danger: "text-fg-muted hover:bg-danger/10 hover:text-danger",
};

const SIZE: Record<Size, string> = {
  sm: "h-7 w-7 rounded-sm",
  md: "h-9 w-9 rounded",
};

// aria-label is REQUIRED by the type: an icon-only button must always be
// labelled, so the a11y gap is closed at the call site by construction.
type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label"> & {
  "aria-label": string;
  variant?: Variant;
  size?: Size;
  children: ReactNode;
};

export const IconButton = forwardRef<HTMLButtonElement, Props>(function IconButton(
  { variant = "ghost", size = "md", className = "", children, type = "button", ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={[
        "inline-flex items-center justify-center shrink-0",
        "transition-colors duration-150",
        "outline-none focus-visible:ring-2 focus-visible:ring-ring/30 focus-visible:ring-offset-1 focus-visible:ring-offset-surface",
        "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none",
        VARIANT[variant],
        SIZE[size],
        className,
      ].join(" ")}
      {...rest}
    >
      {children}
    </button>
  );
});

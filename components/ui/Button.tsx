"use client";

import { ButtonHTMLAttributes, forwardRef, ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "dangerOutline";
type Size = "sm" | "md" | "lg";

const VARIANT: Record<Variant, string> = {
  primary:
    "bg-zinc-900 text-white border border-zinc-900 hover:bg-zinc-800 hover:border-zinc-800",
  secondary:
    "bg-white text-zinc-800 border border-zinc-200 hover:bg-zinc-50 hover:border-zinc-300",
  ghost:
    "bg-transparent text-zinc-600 border border-transparent hover:bg-zinc-100 hover:text-zinc-900",
  danger:
    "bg-red-600 text-white border border-red-600 hover:bg-red-700 hover:border-red-700",
  dangerOutline:
    "bg-white text-red-600 border border-red-200 hover:bg-red-50 hover:border-red-300",
};

const SIZE: Record<Size, string> = {
  sm: "h-8 px-3 text-xs rounded-md gap-1.5",
  md: "h-10 px-3.5 text-sm rounded-md gap-2",
  lg: "h-11 px-4 text-sm rounded-md gap-2",
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
        "inline-flex items-center justify-center whitespace-nowrap font-medium",
        "transition-colors duration-150",
        "outline-none focus-visible:ring-2 focus-visible:ring-zinc-900/15 focus-visible:ring-offset-1 focus-visible:ring-offset-white",
        "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none",
        "select-none",
        VARIANT[variant],
        SIZE[size],
        className,
      ].join(" ")}
      {...rest}
    >
      {loading ? (
        <span
          aria-hidden
          className="inline-block h-3.5 w-3.5 rounded-full border-2 border-current border-r-transparent animate-spin"
        />
      ) : (
        leftIcon
      )}
      {children}
    </button>
  );
});

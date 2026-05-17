"use client";

import { InputHTMLAttributes, forwardRef, ReactNode } from "react";

type Props = Omit<InputHTMLAttributes<HTMLInputElement>, "onChange"> & {
  label?: ReactNode;
  description?: ReactNode;
  errorText?: ReactNode;
  leftSlot?: ReactNode;
  rightSlot?: ReactNode;
  onValueChange?: (value: string) => void;
  onClear?: () => void;
  clearable?: boolean;
};

export const Input = forwardRef<HTMLInputElement, Props>(function Input(
  {
    label,
    description,
    errorText,
    leftSlot,
    rightSlot,
    onValueChange,
    onClear,
    clearable,
    className = "",
    value,
    ...rest
  },
  ref,
) {
  const hasValue = typeof value === "string" ? value.length > 0 : !!value;
  return (
    <label className="block">
      {label && (
        <div className="text-xs font-medium text-zinc-700 mb-1.5">{label}</div>
      )}
      <div
        className={[
          "group flex items-center gap-2",
          "h-10 px-3 rounded-md",
          "bg-white border border-zinc-200",
          "focus-within:border-zinc-400 focus-within:ring-2 focus-within:ring-zinc-900/10",
          "transition-colors",
          errorText ? "border-red-300 focus-within:border-red-400 focus-within:ring-red-500/10" : "",
        ].join(" ")}
      >
        {leftSlot && (
          <span className="shrink-0 text-zinc-400">{leftSlot}</span>
        )}
        <input
          ref={ref}
          value={value}
          onChange={(e) => onValueChange?.(e.target.value)}
          className={[
            "flex-1 min-w-0 bg-transparent outline-none",
            "text-sm text-zinc-900 placeholder:text-zinc-400",
            "disabled:opacity-50 disabled:cursor-not-allowed",
            className,
          ].join(" ")}
          {...rest}
        />
        {clearable && hasValue && (
          <button
            type="button"
            onClick={() => {
              onValueChange?.("");
              onClear?.();
            }}
            className="shrink-0 w-5 h-5 inline-flex items-center justify-center rounded-full text-zinc-400 hover:text-zinc-700 hover:bg-zinc-100"
            aria-label="クリア"
          >
            <svg viewBox="0 0 20 20" className="w-3 h-3" fill="currentColor">
              <path d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z" />
            </svg>
          </button>
        )}
        {rightSlot && <span className="shrink-0">{rightSlot}</span>}
      </div>
      {description && !errorText && (
        <div className="text-xs text-zinc-500 mt-1.5">{description}</div>
      )}
      {errorText && (
        <div className="text-xs text-red-600 mt-1.5">{errorText}</div>
      )}
    </label>
  );
});

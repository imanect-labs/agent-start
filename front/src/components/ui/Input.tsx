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
  inputClassName?: string;
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
    inputClassName = "",
    value,
    ...rest
  },
  ref,
) {
  const hasValue = typeof value === "string" ? value.length > 0 : !!value;
  return (
    <label className={`block ${className}`}>
      {label && <div className="text-xs font-medium text-fg-muted mb-1.5">{label}</div>}
      <div
        className={[
          "group flex items-center gap-2",
          "h-10 px-3 rounded-md",
          "bg-surface border border-line",
          "focus-within:border-line-strong focus-within:ring-2 focus-within:ring-ring/10",
          "transition-colors",
          errorText
            ? "border-danger/40 focus-within:border-danger focus-within:ring-danger/10"
            : "",
        ].join(" ")}
      >
        {leftSlot && <span className="shrink-0 text-fg-faint">{leftSlot}</span>}
        <input
          ref={ref}
          value={value}
          onChange={(e) => onValueChange?.(e.target.value)}
          className={[
            "flex-1 min-w-0 bg-transparent outline-none",
            "text-sm text-fg placeholder:text-fg-faint",
            "disabled:opacity-50 disabled:cursor-not-allowed",
            inputClassName,
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
            className="shrink-0 w-5 h-5 inline-flex items-center justify-center rounded-full text-fg-faint hover:text-fg hover:bg-surface-muted"
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
        <div className="text-xs text-fg-subtle mt-1.5">{description}</div>
      )}
      {errorText && <div className="text-xs text-danger mt-1.5">{errorText}</div>}
    </label>
  );
});

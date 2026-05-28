import { ReactNode } from "react";
import { IconAlertTriangle, IconRefresh } from "../icons";
import { Button } from "./Button";

/**
 * Centered error surface with an alert icon, message, and optional retry.
 * Pass `onRetry` to render the built-in retry button, or `action` for a custom
 * recovery control.
 */
export function ErrorState({
  title = "問題が発生しました",
  description,
  onRetry,
  retryLabel = "再試行",
  action,
  className = "",
  compact = false,
}: {
  title?: ReactNode;
  description?: ReactNode;
  onRetry?: () => void;
  retryLabel?: string;
  action?: ReactNode;
  className?: string;
  compact?: boolean;
}) {
  return (
    <div
      role="alert"
      className={[
        "flex flex-col items-center justify-center text-center",
        compact ? "gap-2 p-6" : "gap-3 p-10",
        className,
      ].join(" ")}
    >
      <span className="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-danger/10 text-danger [&_svg]:w-5 [&_svg]:h-5">
        <IconAlertTriangle />
      </span>
      <div className="text-sm font-medium text-fg">{title}</div>
      {description && <p className="text-xs text-fg-subtle max-w-sm break-words">{description}</p>}
      {action ??
        (onRetry && (
          <Button
            size="sm"
            variant="secondary"
            leftIcon={<IconRefresh className="w-3.5 h-3.5" />}
            onClick={onRetry}
          >
            {retryLabel}
          </Button>
        ))}
    </div>
  );
}

import { ReactNode } from "react";

/**
 * Centered placeholder for "nothing here yet" surfaces: optional icon, title,
 * description, and an action slot (e.g. a Button CTA).
 */
export function EmptyState({
  icon,
  title,
  description,
  action,
  className = "",
  compact = false,
}: {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
  compact?: boolean;
}) {
  return (
    <div
      className={[
        "flex flex-col items-center justify-center text-center",
        compact ? "gap-2 p-6" : "gap-3 p-10",
        className,
      ].join(" ")}
    >
      {icon && (
        <span className="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-surface-muted text-fg-faint [&_svg]:w-5 [&_svg]:h-5">
          {icon}
        </span>
      )}
      <div className="text-sm font-medium text-fg">{title}</div>
      {description && <p className="text-xs text-fg-subtle max-w-xs">{description}</p>}
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}

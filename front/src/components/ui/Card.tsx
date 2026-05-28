import { HTMLAttributes, forwardRef, ReactNode } from "react";

type Elevation = "flat" | "sm" | "md";

const ELEVATION: Record<Elevation, string> = {
  flat: "",
  sm: "shadow-sm",
  md: "shadow-md",
};

type CardProps = HTMLAttributes<HTMLDivElement> & {
  elevation?: Elevation;
  /** Adds hover affordance for clickable cards. */
  interactive?: boolean;
};

/**
 * Surface container with the standard border + radius + elevation. Compose with
 * CardHeader / CardBody / CardFooter, or pass children directly for simple use.
 */
export const Card = forwardRef<HTMLDivElement, CardProps>(function Card(
  { elevation = "sm", interactive = false, className = "", children, ...rest },
  ref,
) {
  return (
    <div
      ref={ref}
      className={[
        "rounded-lg border border-line bg-surface",
        ELEVATION[elevation],
        interactive
          ? "transition-[box-shadow,border-color,background-color] duration-150 hover:border-line-strong hover:shadow-md cursor-pointer"
          : "",
        className,
      ].join(" ")}
      {...rest}
    >
      {children}
    </div>
  );
});

export function CardHeader({
  title,
  description,
  actions,
  className = "",
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={[
        "flex items-start justify-between gap-3 px-4 py-3 border-b border-line",
        className,
      ].join(" ")}
    >
      <div className="min-w-0">
        {title && <div className="text-sm font-semibold text-fg truncate">{title}</div>}
        {description && <div className="text-xs text-fg-subtle mt-0.5">{description}</div>}
      </div>
      {actions && <div className="shrink-0 flex items-center gap-2">{actions}</div>}
    </div>
  );
}

export function CardBody({ className = "", children, ...rest }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={["p-4", className].join(" ")} {...rest}>
      {children}
    </div>
  );
}

export function CardFooter({ className = "", children, ...rest }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={[
        "flex items-center justify-end gap-2 px-4 py-3 border-t border-line",
        className,
      ].join(" ")}
      {...rest}
    >
      {children}
    </div>
  );
}

export function Skeleton({
  className = "",
  style,
}: {
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <div
      aria-hidden
      className={["bg-surface-muted/70 animate-pulse rounded", className].join(" ")}
      style={style}
    />
  );
}

export function SkeletonRows({
  n = 3,
  rowHeight = 28,
  gap = 8,
  className = "",
}: {
  n?: number;
  rowHeight?: number;
  gap?: number;
  className?: string;
}) {
  return (
    <div className={["flex flex-col", className].join(" ")} style={{ gap }}>
      {Array.from({ length: n }).map((_, i) => (
        <Skeleton key={i} style={{ height: rowHeight, width: `${85 - (i % 3) * 10}%` }} />
      ))}
    </div>
  );
}

/** A block of text lines with a shorter trailing line. */
export function SkeletonText({
  lines = 3,
  className = "",
}: {
  lines?: number;
  className?: string;
}) {
  return (
    <div className={["flex flex-col gap-2", className].join(" ")}>
      {Array.from({ length: lines }).map((_, i) => (
        <Skeleton
          key={i}
          className="h-3.5"
          style={{ width: i === lines - 1 ? "55%" : `${92 - (i % 2) * 8}%` }}
        />
      ))}
    </div>
  );
}

/** A bordered card placeholder: title line + a few body lines. */
export function SkeletonCard({ className = "" }: { className?: string }) {
  return (
    <div className={["rounded-lg border border-line bg-surface p-4", className].join(" ")}>
      <Skeleton className="h-4 w-1/3 mb-3" />
      <SkeletonText lines={2} />
    </div>
  );
}

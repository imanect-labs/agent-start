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

"use client";

type Size = "xs" | "sm" | "md";

const SIZE: Record<Size, string> = {
  xs: "h-3 w-3 border-2",
  sm: "h-4 w-4 border-2",
  md: "h-6 w-6 border-[3px]",
};

export function Spinner({
  size = "sm",
  className = "",
}: {
  size?: Size;
  className?: string;
}) {
  return (
    <span
      aria-label="読み込み中"
      role="status"
      className={[
        "inline-block rounded-full border-current border-r-transparent animate-spin",
        "text-zinc-400",
        SIZE[size],
        className,
      ].join(" ")}
    />
  );
}

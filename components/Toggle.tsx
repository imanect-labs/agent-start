"use client";

type Props = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  tone?: "default" | "danger";
};

export function Toggle({ checked, onChange, disabled, tone = "default" }: Props) {
  const onBg = tone === "danger" ? "bg-red-600" : "bg-zinc-900";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={[
        "relative inline-flex h-6 w-10 shrink-0 rounded-full transition-colors duration-150",
        "outline-none focus-visible:ring-2 focus-visible:ring-zinc-900/15 focus-visible:ring-offset-1 focus-visible:ring-offset-white",
        disabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer",
        checked ? onBg : "bg-zinc-200",
      ].join(" ")}
    >
      <span
        className={[
          "absolute top-0.5 inline-block h-5 w-5 rounded-full bg-white shadow-sm",
          "transition-transform duration-150",
          checked ? "translate-x-[1.125rem]" : "translate-x-0.5",
        ].join(" ")}
      />
    </button>
  );
}

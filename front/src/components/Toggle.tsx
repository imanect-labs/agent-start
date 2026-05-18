type Props = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  tone?: "default" | "danger";
};

export function Toggle({ checked, onChange, disabled, tone = "default" }: Props) {
  const onBg = tone === "danger" ? "bg-danger" : "bg-accent";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={[
        "relative inline-flex h-6 w-10 shrink-0 rounded-full transition-colors duration-150",
        "outline-none focus-visible:ring-2 focus-visible:ring-ring/20 focus-visible:ring-offset-1 focus-visible:ring-offset-surface",
        disabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer",
        checked ? onBg : "bg-surface-muted border border-line",
      ].join(" ")}
    >
      <span
        className={[
          "absolute top-0.5 inline-block h-5 w-5 rounded-full shadow-sm",
          "transition-transform duration-150",
          checked ? "bg-accent-fg translate-x-[1.125rem]" : "bg-surface translate-x-0.5",
        ].join(" ")}
      />
    </button>
  );
}

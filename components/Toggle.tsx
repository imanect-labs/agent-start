"use client";

type Props = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  color?: "blue" | "red";
};

export function Toggle({
  checked,
  onChange,
  disabled,
  color = "blue",
}: Props) {
  const onBg = color === "red" ? "bg-red-600" : "bg-blue-600";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-7 w-12 shrink-0 rounded-full transition-colors outline-none focus-visible:ring-2 focus-visible:ring-blue-400 ${
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"
      } ${checked ? onBg : "bg-zinc-300"}`}
    >
      <span
        className={`absolute top-0.5 inline-block h-6 w-6 rounded-full bg-white shadow transition-transform ${
          checked ? "translate-x-[1.375rem]" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

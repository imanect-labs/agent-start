import { ReactNode, useId, useRef, useState } from "react";

type Side = "top" | "bottom" | "left" | "right";

const SIDE: Record<Side, string> = {
  top: "bottom-full left-1/2 -translate-x-1/2 mb-1.5",
  bottom: "top-full left-1/2 -translate-x-1/2 mt-1.5",
  left: "right-full top-1/2 -translate-y-1/2 mr-1.5",
  right: "left-full top-1/2 -translate-y-1/2 ml-1.5",
};

/**
 * Lightweight hover/focus tooltip. Wraps a single interactive child and labels
 * it via aria-describedby. Opens after a short delay on pointer hover; opens
 * immediately on keyboard focus. Motion is handled by the global
 * prefers-reduced-motion guard in globals.css.
 */
export function Tooltip({
  label,
  side = "top",
  delay = 350,
  children,
  className = "",
}: {
  label: ReactNode;
  side?: Side;
  delay?: number;
  children: ReactNode;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const id = useId();

  const clear = () => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  };
  const show = (immediate = false) => {
    clear();
    if (immediate) setOpen(true);
    else timer.current = setTimeout(() => setOpen(true), delay);
  };
  const hide = () => {
    clear();
    setOpen(false);
  };

  return (
    <span
      className={["relative inline-flex", className].join(" ")}
      onPointerEnter={() => show()}
      onPointerLeave={hide}
      onFocusCapture={() => show(true)}
      onBlurCapture={hide}
    >
      <span aria-describedby={open ? id : undefined} className="inline-flex">
        {children}
      </span>
      {open && (
        <span
          role="tooltip"
          id={id}
          className={[
            "pointer-events-none absolute z-50 whitespace-nowrap",
            "rounded-md px-2 py-1 text-2xs font-medium",
            "bg-surface-elev/90 backdrop-blur-xl text-fg border border-line shadow-pop",
            SIDE[side],
          ].join(" ")}
        >
          {label}
        </span>
      )}
    </span>
  );
}

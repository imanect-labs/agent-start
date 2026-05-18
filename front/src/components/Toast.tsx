import { ReactNode, createContext, useCallback, useContext, useState } from "react";

export type ToastColor = "success" | "danger" | "warning" | "info";

type ToastInput = {
  title: string;
  description?: string;
  color?: ToastColor;
  durationMs?: number;
};

type Toast = ToastInput & { id: number };

const ToastCtx = createContext<(t: ToastInput) => void>(() => {});

export function useToast() {
  return useContext(ToastCtx);
}

let nextId = 1;

const DOT: Record<ToastColor, string> = {
  success: "bg-success",
  danger: "bg-danger",
  warning: "bg-warn",
  info: "bg-fg-faint",
};

export function ToastHost({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const show = useCallback((t: ToastInput) => {
    const id = nextId++;
    const dur = t.durationMs ?? 3500;
    setToasts((prev) => [...prev, { ...t, id }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((p) => p.id !== id));
    }, dur);
  }, []);

  return (
    <ToastCtx.Provider value={show}>
      {children}
      <div className="pointer-events-none fixed top-0 left-0 right-0 z-[200] flex flex-col items-center gap-2 px-4 pt-4 safe-top">
        {toasts.map((t) => {
          return (
            <div
              key={t.id}
              role="status"
              className={[
                "pointer-events-auto w-full max-w-md rounded-lg border p-3 pl-3.5",
                "bg-surface-elev border-line text-fg",
                "shadow-[0_12px_32px_-12px_rgba(0,0,0,0.35)]",
                "flex items-start gap-3",
              ].join(" ")}
            >
              <span
                aria-hidden
                className={`mt-1.5 h-2 w-2 rounded-full shrink-0 ${DOT[t.color ?? "info"]}`}
              />
              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm leading-snug">{t.title}</div>
                {t.description && (
                  <div className="text-xs text-fg-subtle mt-0.5 break-all">{t.description}</div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </ToastCtx.Provider>
  );
}

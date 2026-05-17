"use client";

import {
  ReactNode,
  createContext,
  useCallback,
  useContext,
  useState,
} from "react";

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

const STYLES: Record<ToastColor, string> = {
  success: "bg-emerald-50 border-emerald-300 text-emerald-900",
  danger: "bg-red-50 border-red-300 text-red-900",
  warning: "bg-amber-50 border-amber-300 text-amber-900",
  info: "bg-blue-50 border-blue-300 text-blue-900",
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
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`pointer-events-auto w-full max-w-md rounded-xl border shadow-lg p-3 ${
              STYLES[t.color ?? "info"]
            }`}
          >
            <div className="font-semibold text-sm">{t.title}</div>
            {t.description && (
              <div className="text-xs mt-0.5 break-all opacity-90">
                {t.description}
              </div>
            )}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

"use client";

import { ReactNode } from "react";
import { ToastHost } from "@/components/Toast";
import { ThemeProvider } from "@/components/ThemeProvider";

export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider>
      <ToastHost>{children}</ToastHost>
    </ThemeProvider>
  );
}

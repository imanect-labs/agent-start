"use client";

import { ReactNode } from "react";
import { ToastHost } from "@/components/Toast";

export function Providers({ children }: { children: ReactNode }) {
  return <ToastHost>{children}</ToastHost>;
}

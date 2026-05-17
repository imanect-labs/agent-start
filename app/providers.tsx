"use client";

import { HeroUIProvider } from "@heroui/react";
import { ReactNode } from "react";
import { ToastHost } from "@/components/Toast";

export function Providers({ children }: { children: ReactNode }) {
  return (
    <HeroUIProvider>
      <ToastHost>{children}</ToastHost>
    </HeroUIProvider>
  );
}

import { Outlet } from "@tanstack/react-router";
import { ThemeProvider } from "@/components/ThemeProvider";
import { ToastHost } from "@/components/Toast";

export function RootLayout() {
  return (
    <ThemeProvider>
      <ToastHost>
        <Outlet />
      </ToastHost>
    </ThemeProvider>
  );
}

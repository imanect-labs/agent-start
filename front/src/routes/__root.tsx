import { Outlet } from "@tanstack/react-router";
import { ThemeProvider } from "@/components/ThemeProvider";
import { ToastHost } from "@/components/Toast";
import { UpdateBanner } from "@/components/UpdateBanner";

export function RootLayout() {
  return (
    <ThemeProvider>
      <ToastHost>
        <UpdateBanner />
        <Outlet />
      </ToastHost>
    </ThemeProvider>
  );
}

import type { Metadata, Viewport } from "next";
import { Providers } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
  title: "agent-start",
  description: "claude code / codex launcher",
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
};

// Runs before hydration so the correct theme class is applied on first paint.
const themeBootScript = `(() => {
  try {
    var v = localStorage.getItem('agent-start:theme');
    var dark = v === 'dark' || ((v === null || v === 'system') &&
      window.matchMedia('(prefers-color-scheme: dark)').matches);
    if (dark) document.documentElement.classList.add('dark');
  } catch (e) {}
})();`;

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="ja" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeBootScript }} />
      </head>
      <body className="min-h-screen bg-app text-fg">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}

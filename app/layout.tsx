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
  themeColor: "#fafafa",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="ja">
      <body className="min-h-screen bg-zinc-50 text-zinc-900">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}

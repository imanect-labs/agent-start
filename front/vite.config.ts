import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

const HOST = "http://127.0.0.1:3030";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    port: 5173,
    host: "0.0.0.0",
    proxy: {
      "/api": { target: HOST, changeOrigin: false },
      "/v1": { target: HOST, changeOrigin: false },
      "/ws": { target: HOST, changeOrigin: false, ws: true },
      // code-server reverse proxy (HTTP + WS). Without this, navigating to
      // /v/<session>/ in dev would hit the SPA's 404 instead of the host.
      "/v/": { target: HOST, changeOrigin: false, ws: true },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});

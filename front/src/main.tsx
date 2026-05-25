import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import { SWRConfig } from "swr";
import { router } from "./router";
import "./styles/globals.css";

const SWR_CACHE_KEY = "agent-start:swr-cache:v1";
// Keys whose responses we mirror to localStorage so a reload paints content
// immediately while SWR revalidates in the background. Keep this small —
// volatile or large payloads shouldn't be persisted.
const PERSIST_KEYS = new Set<string>(["/api/projects", "/api/sessions"]);

type CacheValue = { data?: unknown };

function loadCache(): Record<string, CacheValue> {
  try {
    const raw = localStorage.getItem(SWR_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const out: Record<string, CacheValue> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (PERSIST_KEYS.has(k)) out[k] = { data: v };
    }
    return out;
  } catch {
    return {};
  }
}

function persistCache(map: Map<string, CacheValue>) {
  try {
    const obj: Record<string, unknown> = {};
    for (const k of PERSIST_KEYS) {
      const v = map.get(k);
      if (v?.data !== undefined) obj[k] = v.data;
    }
    localStorage.setItem(SWR_CACHE_KEY, JSON.stringify(obj));
  } catch {
    // quota / serialization — best-effort
  }
}

const initialCache = loadCache();
const fallback: Record<string, unknown> = {};
for (const [k, v] of Object.entries(initialCache)) {
  if (v.data !== undefined) fallback[k] = v.data;
}

function provider(): Map<string, CacheValue> {
  const map = new Map<string, CacheValue>(Object.entries(initialCache));
  if (typeof window !== "undefined") {
    window.addEventListener("beforeunload", () => persistCache(map));
    // Periodically persist as a safety net for browsers that skip
    // beforeunload (e.g. mobile tab discards).
    setInterval(() => persistCache(map), 15000);
  }
  return map;
}

const container = document.getElementById("root");
if (!container) throw new Error("#root not found");

createRoot(container).render(
  <StrictMode>
    <SWRConfig value={{ fallback, provider }}>
      <RouterProvider router={router} />
    </SWRConfig>
  </StrictMode>,
);

"use client";

import { useEffect, useRef, useState } from "react";
import "@xterm/xterm/css/xterm.css";
import { Button } from "@/components/ui";

type Props = {
  sessionName: string;
};

const TERM_THEME = {
  background: "#09090b",
  foreground: "#fafafa",
  cursor: "#fafafa",
  cursorAccent: "#09090b",
  selectionBackground: "#3f3f46",
  black: "#27272a",
  red: "#f87171",
  green: "#4ade80",
  yellow: "#facc15",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#22d3ee",
  white: "#fafafa",
  brightBlack: "#52525b",
  brightRed: "#fca5a5",
  brightGreen: "#86efac",
  brightYellow: "#fde68a",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#67e8f9",
  brightWhite: "#ffffff",
};

type Status = "connecting" | "open" | "closed";

export function Terminal({ sessionName }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const termRef = useRef<any>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fitRef = useRef<any>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const [attempt, setAttempt] = useState(0);
  const ctrlPendingRef = useRef(false);
  const [ctrlActive, setCtrlActive] = useState(false);

  // Single effect: bootstrap xterm + open WS, tear both down together.
  useEffect(() => {
    let disposed = false;
    let resizeObs: ResizeObserver | null = null;
    let ws: WebSocket | null = null;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let term: any = null;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let fit: any = null;

    setStatus("connecting");

    (async () => {
      const [{ Terminal: XTerm }, { FitAddon }] = await Promise.all([
        import("@xterm/xterm"),
        import("@xterm/addon-fit"),
      ]);
      if (disposed || !containerRef.current) return;

      term = new XTerm({
        cursorBlink: true,
        fontFamily:
          'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
        fontSize: 13,
        lineHeight: 1.2,
        theme: TERM_THEME,
        scrollback: 5000,
        allowProposedApi: true,
        convertEol: false,
      });
      fit = new FitAddon();
      term.loadAddon(fit);
      term.open(containerRef.current);
      try {
        fit.fit();
      } catch {
        // ignore
      }

      termRef.current = term;
      fitRef.current = fit;

      term.onData((data: string) => {
        // sticky Ctrl: convert next ASCII letter to control byte
        if (ctrlPendingRef.current && data.length === 1) {
          const c = data.charCodeAt(0);
          let ctrlByte: number | null = null;
          if (c >= 0x61 && c <= 0x7a) ctrlByte = c - 0x60;
          else if (c >= 0x41 && c <= 0x5a) ctrlByte = c - 0x40;
          if (ctrlByte != null) {
            ctrlPendingRef.current = false;
            setCtrlActive(false);
            sendInputViaWs(ws, String.fromCharCode(ctrlByte));
            return;
          }
        }
        sendInputViaWs(ws, data);
      });

      term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
        if (ws && ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: "resize", cols, rows }));
        }
      });

      resizeObs = new ResizeObserver(() => {
        try {
          fit.fit();
        } catch {
          // ignore
        }
      });
      resizeObs.observe(containerRef.current);

      // Touch-swipe scroll: vertical pan inside terminal viewport.
      const ROW_PX = 22;
      let touchY: number | null = null;
      let accum = 0;
      const onTouchStart = (e: TouchEvent) => {
        if (e.touches.length !== 1) {
          touchY = null;
          return;
        }
        touchY = e.touches[0].clientY;
        accum = 0;
      };
      const onTouchMove = (e: TouchEvent) => {
        if (touchY == null || e.touches.length !== 1) return;
        const y = e.touches[0].clientY;
        const dy = y - touchY;
        touchY = y;
        accum += dy;
        const lines = Math.trunc(accum / ROW_PX);
        if (lines !== 0) {
          accum -= lines * ROW_PX;
          // finger down (positive dy) → reveal content above → scroll up (-lines)
          dispatchScroll(term, ws, -lines);
          e.preventDefault();
        }
      };
      const onTouchEnd = () => {
        touchY = null;
      };
      containerRef.current.addEventListener("touchstart", onTouchStart, { passive: true });
      containerRef.current.addEventListener("touchmove", onTouchMove, { passive: false });
      containerRef.current.addEventListener("touchend", onTouchEnd, { passive: true });
      containerRef.current.addEventListener("touchcancel", onTouchEnd, { passive: true });
      (containerRef.current as HTMLDivElement & { __touchCleanup?: () => void }).__touchCleanup = () => {
        containerRef.current?.removeEventListener("touchstart", onTouchStart);
        containerRef.current?.removeEventListener("touchmove", onTouchMove);
        containerRef.current?.removeEventListener("touchend", onTouchEnd);
        containerRef.current?.removeEventListener("touchcancel", onTouchEnd);
      };

      // Open WebSocket now that xterm is ready
      const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${proto}//${window.location.host}/ws/terminal?session=${encodeURIComponent(sessionName)}`;
      ws = new WebSocket(url);
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;

      ws.onopen = () => {
        if (disposed) return;
        setStatus("open");
        try {
          fit?.fit();
          if (term && ws && ws.readyState === WebSocket.OPEN) {
            ws.send(
              JSON.stringify({
                type: "resize",
                cols: term.cols,
                rows: term.rows,
              }),
            );
          }
        } catch {
          // ignore
        }
        term?.focus();
      };
      ws.onmessage = (ev) => {
        if (!term) return;
        if (typeof ev.data === "string") {
          term.write(ev.data);
        } else if (ev.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(ev.data));
        }
      };
      ws.onclose = () => {
        if (disposed) return;
        setStatus("closed");
      };
      ws.onerror = () => {
        if (disposed) return;
        setStatus("closed");
      };
    })();

    return () => {
      disposed = true;
      if (resizeObs) resizeObs.disconnect();
      const cleanup = (
        containerRef.current as
          | (HTMLDivElement & { __touchCleanup?: () => void })
          | null
      )?.__touchCleanup;
      if (cleanup) cleanup();
      try {
        ws?.close();
      } catch {
        // ignore
      }
      try {
        term?.dispose?.();
      } catch {
        // ignore
      }
      termRef.current = null;
      fitRef.current = null;
      wsRef.current = null;
    };
  }, [sessionName, attempt]);

  const focusTerm = () => {
    termRef.current?.focus();
  };

  const handleVirtualKey = (data: string) => {
    if (data === "__ctrl__") {
      ctrlPendingRef.current = !ctrlPendingRef.current;
      setCtrlActive(ctrlPendingRef.current);
      focusTerm();
      return;
    }
    sendInputViaWs(wsRef.current, data);
    focusTerm();
  };

  const handleScroll = (direction: -1 | 1) => {
    const term = termRef.current;
    if (!term) return;
    const visible = term.rows ?? 10;
    // ~one screen worth of lines per button tap
    const lines = direction * Math.max(5, visible - 2);
    dispatchScroll(term, wsRef.current, lines);
  };

  return (
    <div className="flex flex-col gap-2 h-full min-h-0">
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5">
          <span
            className={[
              "inline-block w-1.5 h-1.5 rounded-full",
              status === "open"
                ? "bg-emerald-500"
                : status === "connecting"
                  ? "bg-amber-500"
                  : "bg-red-500",
            ].join(" ")}
          />
          <span className="text-zinc-500">
            {status === "open"
              ? "接続中"
              : status === "connecting"
                ? "接続中…"
                : "切断されました"}
          </span>
        </div>
        {status === "closed" && (
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setAttempt((n) => n + 1)}
          >
            再接続
          </Button>
        )}
      </div>

      <div
        ref={containerRef}
        onClick={focusTerm}
        className="rounded-md bg-zinc-950 border border-zinc-200 p-2 overflow-hidden flex-1 min-h-0"
        style={{ touchAction: "none" }}
      />

      <VirtualKeys
        onKey={handleVirtualKey}
        onScroll={handleScroll}
        ctrlActive={ctrlActive}
      />
    </div>
  );
}

function sendInputViaWs(ws: WebSocket | null, data: string) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: "input", data }));
  }
}

// Scroll uses tmux copy-mode on the server side so it works for any pane
// (claude/TUI included), and does not collide with the running app's
// keybindings (e.g. bash readline history-search on PgUp/PgDn).
// Direction: negative = up (older), positive = down (newer).
function dispatchScroll(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  _term: any,
  ws: WebSocket | null,
  lines: number,
) {
  if (!ws || ws.readyState !== WebSocket.OPEN || lines === 0) return;
  ws.send(
    JSON.stringify({
      type: "scroll",
      direction: lines < 0 ? -1 : 1,
      count: Math.abs(lines),
    }),
  );
}

type KeyDef =
  | { kind: "send"; label: string; data: string }
  | { kind: "toggle"; label: string; active: boolean }
  | { kind: "scroll"; label: string; direction: -1 | 1; tone: "scroll" };

function VirtualKeys({
  onKey,
  onScroll,
  ctrlActive,
}: {
  onKey: (data: string) => void;
  onScroll: (direction: -1 | 1) => void;
  ctrlActive: boolean;
}) {
  const keys: KeyDef[] = [
    { kind: "scroll", label: "↥", direction: -1, tone: "scroll" },
    { kind: "scroll", label: "↧", direction: 1, tone: "scroll" },
    { kind: "send", label: "Esc", data: "\x1b" },
    { kind: "send", label: "Tab", data: "\t" },
    { kind: "toggle", label: "Ctrl", active: ctrlActive },
    { kind: "send", label: "↑", data: "\x1b[A" },
    { kind: "send", label: "↓", data: "\x1b[B" },
    { kind: "send", label: "←", data: "\x1b[D" },
    { kind: "send", label: "→", data: "\x1b[C" },
    { kind: "send", label: "Enter", data: "\r" },
  ];
  return (
    <div className="flex gap-1.5 overflow-x-auto -mx-1 px-1 pb-1">
      {keys.map((k, i) => {
        const active = k.kind === "toggle" && k.active;
        const isScroll = k.kind === "scroll";
        return (
          <button
            key={i}
            type="button"
            onClick={() => {
              if (k.kind === "send") onKey(k.data);
              else if (k.kind === "toggle") onKey("__ctrl__");
              else onScroll(k.direction);
            }}
            className={[
              "shrink-0 inline-flex items-center justify-center",
              "min-h-9 px-3 rounded-md text-xs font-medium",
              "border transition-colors",
              "select-none touch-manipulation",
              active
                ? "bg-zinc-900 text-white border-zinc-900"
                : isScroll
                  ? "bg-zinc-50 text-zinc-700 border-zinc-200 hover:bg-zinc-100"
                  : "bg-white text-zinc-700 border-zinc-200 hover:bg-zinc-50",
              isScroll ? "min-w-9" : "",
            ].join(" ")}
          >
            {k.label}
          </button>
        );
      })}
    </div>
  );
}

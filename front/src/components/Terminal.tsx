import { useEffect, useMemo, useRef, useState } from "react";
import "@xterm/xterm/css/xterm.css";
import { Button } from "@/components/ui";
import { useTheme } from "@/components/ThemeProvider";

type Props = {
  sessionName: string;
  /** tmux window index to attach to. Defaults to 0 (first window). */
  windowId?: number;
  /** show the on-screen Ctrl/Esc/arrows row. Defaults to "auto" (touch only). */
  virtualKeys?: "auto" | "always" | "never";
  /** When true, suppress auto-reconnect — host has the session marked
   *  stopped, so retrying the WS just 404-loops. The parent should bump
   *  this terminal's key once the session is restarted to remount. */
  stopped?: boolean;
  /** Called when the user clicks the in-terminal "再開" overlay button.
   *  Only meaningful when `stopped` is true. */
  onRestart?: () => void;
  /** True while a restart RPC is in flight — disables the button. */
  restarting?: boolean;
};

const LIGHT_TERM_THEME = {
  background: "#fafafa",
  foreground: "#18181b",
  cursor: "#18181b",
  cursorAccent: "#fafafa",
  selectionBackground: "#d4d4d8",
  black: "#27272a",
  red: "#dc2626",
  green: "#16a34a",
  yellow: "#ca8a04",
  blue: "#2563eb",
  magenta: "#9333ea",
  cyan: "#0891b2",
  white: "#71717a",
  brightBlack: "#52525b",
  brightRed: "#ef4444",
  brightGreen: "#22c55e",
  brightYellow: "#eab308",
  brightBlue: "#3b82f6",
  brightMagenta: "#a855f7",
  brightCyan: "#06b6d4",
  brightWhite: "#18181b",
};

const DARK_TERM_THEME = {
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

export function Terminal({
  sessionName,
  windowId = 0,
  virtualKeys = "auto",
  stopped = false,
  onRestart,
  restarting = false,
}: Props) {
  const stoppedRef = useRef(stopped);
  useEffect(() => {
    stoppedRef.current = stopped;
  }, [stopped]);
  const { resolved } = useTheme();
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

  const theme = useMemo(
    () => (resolved === "dark" ? DARK_TERM_THEME : LIGHT_TERM_THEME),
    [resolved],
  );

  // Update xterm theme live when the resolved theme changes.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    try {
      term.options.theme = theme;
    } catch {
      // ignore
    }
  }, [theme]);

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
        theme,
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
          dispatchScroll(ws, -lines);
          e.preventDefault();
        }
      };
      const onTouchEnd = () => {
        touchY = null;
      };
      containerRef.current.addEventListener("touchstart", onTouchStart, {
        passive: true,
      });
      containerRef.current.addEventListener("touchmove", onTouchMove, {
        passive: false,
      });
      containerRef.current.addEventListener("touchend", onTouchEnd, {
        passive: true,
      });
      containerRef.current.addEventListener("touchcancel", onTouchEnd, {
        passive: true,
      });
      (
        containerRef.current as HTMLDivElement & {
          __touchCleanup?: () => void;
        }
      ).__touchCleanup = () => {
        containerRef.current?.removeEventListener("touchstart", onTouchStart);
        containerRef.current?.removeEventListener("touchmove", onTouchMove);
        containerRef.current?.removeEventListener("touchend", onTouchEnd);
        containerRef.current?.removeEventListener("touchcancel", onTouchEnd);
      };

      const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url =
        `${proto}//${window.location.host}/ws/terminal` +
        `?session=${encodeURIComponent(sessionName)}` +
        `&window=${encodeURIComponent(String(windowId))}`;
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
      const scheduleReconnect = () => {
        if (disposed) return;
        // When the session is stopped on the host, retrying just hits
        // 404 (window>0) or replays the same snapshot forever (window
        // 0). The user re-enters via the "再開" button, which bumps our
        // key and remounts.
        if (stoppedRef.current) return;
        // Auto-retry with a small linear backoff so a host restart
        // recovers without the user clicking 再接続.
        setTimeout(
          () => {
            if (disposed) return;
            setAttempt((n) => n + 1);
          },
          Math.min(8000, 1500 + 500 * attempt),
        );
      };
      ws.onclose = () => {
        if (disposed) return;
        setStatus("closed");
        scheduleReconnect();
      };
      ws.onerror = () => {
        if (disposed) return;
        setStatus("closed");
        scheduleReconnect();
      };
    })();

    return () => {
      disposed = true;
      if (resizeObs) resizeObs.disconnect();
      const cleanup = (
        containerRef.current as (HTMLDivElement & { __touchCleanup?: () => void }) | null
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
    // theme intentionally excluded: live-updated by the dedicated effect above
    // so we don't tear down the whole terminal on a color change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionName, windowId, attempt]);

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
    const lines = direction * Math.max(5, visible - 2);
    dispatchScroll(wsRef.current, lines);
  };

  const vkClass =
    virtualKeys === "always"
      ? "flex"
      : virtualKeys === "never"
        ? "hidden"
        : // auto: show only on coarse pointer (touch)
          "hidden [@media(pointer:coarse)]:flex";

  return (
    <div className="flex flex-col gap-2 h-full min-h-0">
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5">
          <span
            className={[
              "inline-block w-1.5 h-1.5 rounded-full",
              stopped
                ? "bg-warn"
                : status === "open"
                  ? "bg-success"
                  : status === "connecting"
                    ? "bg-warn"
                    : "bg-danger",
            ].join(" ")}
          />
          <span className="text-fg-subtle">
            {stopped
              ? "停止中 (再起動後のスナップショット)"
              : status === "open"
                ? "接続中"
                : status === "connecting"
                  ? "接続中…"
                  : "切断されました"}
          </span>
        </div>
        {!stopped && status === "closed" && (
          <Button variant="secondary" size="sm" onClick={() => setAttempt((n) => n + 1)}>
            再接続
          </Button>
        )}
      </div>

      <div className="relative flex-1 min-h-0">
        <div
          ref={containerRef}
          onClick={focusTerm}
          className="rounded-md border border-line p-2 overflow-hidden h-full"
          style={{
            touchAction: "none",
            background: theme.background,
          }}
        />
        {stopped && onRestart && (
          // Overlay anchored to the bottom of the terminal pane. Using
          // an overlay (rather than replacing the terminal) keeps the
          // restored scrollback visible above so the user can read the
          // last state while deciding to revive.
          <div className="absolute inset-x-0 bottom-0 p-3 pointer-events-none flex justify-center">
            <div className="pointer-events-auto bg-surface-elev border border-line-strong rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 max-w-[28rem]">
              <div className="text-xs text-fg-muted flex-1">
                セッションは停止しています。
                <br />
                再開すると新しい PTY を起動します。
              </div>
              <Button variant="primary" size="sm" disabled={restarting} onClick={onRestart}>
                {restarting ? "再開中…" : "再開"}
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className={`${vkClass} gap-1.5 overflow-x-auto -mx-1 px-1 pb-1 safe-bottom`}>
        <VirtualKeys onKey={handleVirtualKey} onScroll={handleScroll} ctrlActive={ctrlActive} />
      </div>
    </div>
  );
}

function sendInputViaWs(ws: WebSocket | null, data: string) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: "input", data }));
  }
}

function dispatchScroll(ws: WebSocket | null, lines: number) {
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
    <>
      {keys.map((k, i) => {
        const active = k.kind === "toggle" && k.active;
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
                ? "bg-accent text-accent-fg border-accent"
                : "bg-surface text-fg-muted border-line hover:bg-surface-muted",
            ].join(" ")}
          >
            {k.label}
          </button>
        );
      })}
    </>
  );
}

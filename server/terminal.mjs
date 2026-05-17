import { WebSocketServer } from "ws";
import pty from "node-pty";
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

function configPath() {
  if (process.env.AGENT_START_CONFIG) return process.env.AGENT_START_CONFIG;
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
  return path.join(xdg, "agent-start", "config.json");
}

function loadSessionPrefix() {
  try {
    const raw = readFileSync(configPath(), "utf8");
    const cfg = JSON.parse(raw);
    return typeof cfg.sessionPrefix === "string" ? cfg.sessionPrefix : "cc-";
  } catch {
    return "cc-";
  }
}

const SESSION_PREFIX = loadSessionPrefix();
const SESSION_NAME_RE = /^[A-Za-z0-9_\-]+$/;

function isValidSessionName(name) {
  return (
    typeof name === "string" &&
    SESSION_NAME_RE.test(name) &&
    name.startsWith(SESSION_PREFIX)
  );
}

async function tmuxHasSession(name) {
  try {
    await execFileP("tmux", ["has-session", "-t", name]);
    return true;
  } catch {
    return false;
  }
}

async function tmuxHasWindow(name, windowId) {
  try {
    await execFileP("tmux", ["display-message", "-p", "-t", `${name}:${windowId}`, "#{window_id}"]);
    return true;
  } catch {
    return false;
  }
}

async function tmuxScroll(name, windowId, direction, count) {
  const dirCmd = direction < 0 ? "scroll-up" : "scroll-down";
  const n = Math.max(1, Math.min(1000, Number(count) || 1));
  const target = `${name}:${windowId}`;
  try {
    await execFileP("tmux", ["copy-mode", "-t", target]);
    await execFileP("tmux", [
      "send-keys",
      "-t",
      target,
      "-N",
      String(n),
      "-X",
      dirCmd,
    ]);
  } catch {
    // ignore (copy-mode might not be valid for this pane state)
  }
}

async function handleConnection(ws, name, windowId) {
  const target = `${name}:${windowId}`;

  // Prep: select the desired window (so the attached client lands on it) and
  // defensively exit any leftover copy-mode in that pane. Without this step,
  // a pane that was left in vi-style copy-mode by a previous client makes the
  // next user's keystrokes (e.g. `f` → "jump forward") look like garbage.
  try {
    await execFileP("tmux", ["select-window", "-t", target]);
  } catch (err) {
    ws.send(JSON.stringify({ type: "error", message: `select-window failed: ${err.message ?? err}` }));
    try { ws.close(1011); } catch {}
    return;
  }
  await execFileP("tmux", ["send-keys", "-t", target, "-X", "cancel"]).catch(
    () => {},
  );

  const term = pty.spawn(
    "tmux",
    ["attach-session", "-d", "-t", name],
    {
      name: "xterm-256color",
      cols: 80,
      rows: 24,
      cwd: process.env.HOME,
      env: { ...process.env, TERM: "xterm-256color" },
    },
  );

  let alive = true;
  // True while we have placed the pane into copy-mode via a scroll request.
  // The next user input must first cancel copy-mode so keystrokes reach the app.
  let inCopyMode = false;
  // Serialize tmux command operations (scroll / cancel) per-connection so that
  // ordering between async tmux calls and term.write is well-defined.
  let chain = Promise.resolve();

  term.onData((data) => {
    if (ws.readyState === ws.OPEN) {
      ws.send(data, { binary: true });
    }
  });

  term.onExit(() => {
    alive = false;
    try {
      ws.close();
    } catch {
      // ignore
    }
  });

  ws.on("message", (raw, isBinary) => {
    if (!alive || isBinary) return;
    let msg;
    try {
      msg = JSON.parse(raw.toString("utf8"));
    } catch {
      return;
    }
    if (msg.type === "input" && typeof msg.data === "string") {
      chain = chain
        .then(async () => {
          if (inCopyMode) {
            try {
              await execFileP("tmux", [
                "send-keys",
                "-t",
                target,
                "-X",
                "cancel",
              ]);
            } catch {
              // ignore
            }
            inCopyMode = false;
          }
          term.write(msg.data);
        })
        .catch(() => {});
    } else if (
      msg.type === "resize" &&
      Number.isFinite(msg.cols) &&
      Number.isFinite(msg.rows)
    ) {
      try {
        term.resize(
          Math.max(1, Math.floor(msg.cols)),
          Math.max(1, Math.floor(msg.rows)),
        );
      } catch {
        // ignore resize failures
      }
    } else if (
      msg.type === "scroll" &&
      (msg.direction === -1 || msg.direction === 1)
    ) {
      chain = chain
        .then(async () => {
          await tmuxScroll(name, windowId, msg.direction, msg.count);
          inCopyMode = true;
        })
        .catch(() => {});
    }
  });

  const cleanup = () => {
    if (!alive) return;
    alive = false;
    try {
      term.kill();
    } catch {
      // ignore
    }
  };
  ws.on("close", cleanup);
  ws.on("error", cleanup);
}

export function attachTerminalWs(httpServer) {
  const wss = new WebSocketServer({ noServer: true });
  return async (req, socket, head) => {
    let url;
    try {
      url = new URL(req.url ?? "/", "http://x");
    } catch {
      socket.destroy();
      return;
    }
    if (url.pathname !== "/ws/terminal") return false;

    const name = url.searchParams.get("session");
    if (!isValidSessionName(name)) {
      socket.write(
        "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n",
      );
      socket.destroy();
      return true;
    }
    if (!(await tmuxHasSession(name))) {
      socket.write(
        "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n",
      );
      socket.destroy();
      return true;
    }
    // Window index defaults to 0 for backwards compatibility / single-tab use.
    const rawWindow = url.searchParams.get("window");
    const windowId = rawWindow == null ? 0 : Number(rawWindow);
    if (!Number.isInteger(windowId) || windowId < 0 || windowId > 9999) {
      socket.write("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
      socket.destroy();
      return true;
    }
    if (!(await tmuxHasWindow(name, windowId))) {
      socket.write("HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
      socket.destroy();
      return true;
    }
    wss.handleUpgrade(req, socket, head, (ws) =>
      handleConnection(ws, name, windowId),
    );
    return true;
  };
}

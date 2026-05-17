import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

export type TmuxSession = {
  name: string;
  createdAt: number;
  attached: boolean;
  path: string;
  cli: string;
  worktreePath: string;
  origPath: string;
};

const SESSION_NAME_RE = /^[A-Za-z0-9_\-]+$/;

export function isValidSessionName(name: string): boolean {
  return SESSION_NAME_RE.test(name);
}

export async function listSessions(prefix: string): Promise<TmuxSession[]> {
  try {
    const { stdout } = await execFileP("tmux", [
      "list-sessions",
      "-F",
      "#{session_name}\t#{session_created}\t#{session_attached}\t#{session_path}\t#{@cli}\t#{@worktree}\t#{@origpath}",
    ]);
    const lines = stdout.split("\n").filter(Boolean);
    return lines
      .map((line) => {
        const [name, created, attached, p, cli, worktreePath, origPath] =
          line.split("\t");
        return {
          name,
          createdAt: Number(created) * 1000,
          attached: attached === "1",
          path: p ?? "",
          cli: cli ?? "",
          worktreePath: worktreePath ?? "",
          origPath: origPath ?? "",
        };
      })
      .filter((s) => s.name.startsWith(prefix))
      .sort((a, b) => b.createdAt - a.createdAt);
  } catch (err) {
    const stderr = (err as { stderr?: string }).stderr ?? "";
    if (/no server running/i.test(stderr) || /no sessions/i.test(stderr)) {
      return [];
    }
    throw err;
  }
}

export async function getSession(
  name: string,
): Promise<TmuxSession | null> {
  if (!isValidSessionName(name)) return null;
  try {
    const { stdout } = await execFileP("tmux", [
      "display-message",
      "-p",
      "-t",
      name,
      "#{session_name}\t#{session_created}\t#{session_attached}\t#{session_path}\t#{@cli}\t#{@worktree}\t#{@origpath}",
    ]);
    const [n, created, attached, p, cli, worktreePath, origPath] = stdout
      .trim()
      .split("\t");
    if (!n) return null;
    return {
      name: n,
      createdAt: Number(created) * 1000,
      attached: attached === "1",
      path: p ?? "",
      cli: cli ?? "",
      worktreePath: worktreePath ?? "",
      origPath: origPath ?? "",
    };
  } catch {
    return null;
  }
}

export async function sessionExists(name: string): Promise<boolean> {
  if (!isValidSessionName(name)) return false;
  try {
    await execFileP("tmux", ["has-session", "-t", name]);
    return true;
  } catch {
    return false;
  }
}

export async function createSession(opts: {
  name: string;
  cwd: string;
  shell: string;
  command?: string;
}): Promise<void> {
  if (!isValidSessionName(opts.name)) {
    throw new Error(`invalid session name: ${opts.name}`);
  }
  const args = ["new-session", "-d", "-s", opts.name, "-c", opts.cwd];
  if (opts.command) {
    // Run via `bash -lc <cmd>` so PATH/.bashrc are sourced.
    args.push(opts.shell, "-lc", opts.command);
  }
  // If no command, tmux uses its default-shell — an interactive shell only.
  await execFileP("tmux", args);
}

export async function setSessionOption(
  name: string,
  key: string,
  value: string,
): Promise<void> {
  if (!isValidSessionName(name)) {
    throw new Error(`invalid session name: ${name}`);
  }
  if (!/^@[a-z0-9_]+$/i.test(key)) {
    throw new Error(`invalid option key: ${key}`);
  }
  await execFileP("tmux", ["set-option", "-t", name, key, value]);
}

export async function killSession(name: string): Promise<void> {
  if (!isValidSessionName(name)) {
    throw new Error(`invalid session name: ${name}`);
  }
  await execFileP("tmux", ["kill-session", "-t", name]);
}

export async function capturePane(name: string, lines = 200): Promise<string> {
  if (!isValidSessionName(name)) {
    throw new Error(`invalid session name: ${name}`);
  }
  const { stdout } = await execFileP("tmux", [
    "capture-pane",
    "-p",
    "-t",
    `${name}:0`,
    "-S",
    `-${lines}`,
  ]);
  return stdout;
}

export type TmuxWindow = {
  index: number;
  name: string;
  active: boolean;
  panes: number;
};

export async function listWindows(name: string): Promise<TmuxWindow[]> {
  if (!isValidSessionName(name)) {
    throw new Error(`invalid session name: ${name}`);
  }
  try {
    const { stdout } = await execFileP("tmux", [
      "list-windows",
      "-t",
      name,
      "-F",
      "#{window_index}\t#{window_name}\t#{window_active}\t#{window_panes}",
    ]);
    return stdout
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [index, n, active, panes] = line.split("\t");
        return {
          index: Number(index),
          name: n ?? "",
          active: active === "1",
          panes: Number(panes) || 1,
        };
      })
      .sort((a, b) => a.index - b.index);
  } catch (err) {
    const stderr = (err as { stderr?: string }).stderr ?? "";
    if (/no server running/i.test(stderr) || /can't find session/i.test(stderr)) {
      return [];
    }
    throw err;
  }
}

export async function newWindow(opts: {
  session: string;
  cwd?: string;
  shell?: string;
  command?: string;
  windowName?: string;
}): Promise<number> {
  if (!isValidSessionName(opts.session)) {
    throw new Error(`invalid session name: ${opts.session}`);
  }
  const args = [
    "new-window",
    "-d",
    "-P",
    "-F",
    "#{window_index}",
    "-t",
    `${opts.session}:`,
  ];
  if (opts.cwd) args.push("-c", opts.cwd);
  if (opts.windowName) args.push("-n", opts.windowName);
  if (opts.command && opts.shell) {
    args.push(opts.shell, "-lc", opts.command);
  }
  const { stdout } = await execFileP("tmux", args);
  const idx = Number(stdout.trim());
  if (!Number.isFinite(idx)) {
    throw new Error(`new-window returned unexpected output: ${stdout}`);
  }
  return idx;
}

export async function killWindow(
  name: string,
  windowIndex: number,
): Promise<void> {
  if (!isValidSessionName(name)) {
    throw new Error(`invalid session name: ${name}`);
  }
  if (!Number.isInteger(windowIndex) || windowIndex < 0) {
    throw new Error(`invalid window index: ${windowIndex}`);
  }
  await execFileP("tmux", ["kill-window", "-t", `${name}:${windowIndex}`]);
}

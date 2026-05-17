import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import { promises as fs } from "node:fs";
import { isPathUnderRoots } from "./projects";
import { loadConfig, worktreeRoot } from "./config";
import { listSessions } from "./tmux";

const execFileP = promisify(execFile);

// Cap large outputs to avoid memory blowups when reading huge diffs.
const MAX_DIFF_BYTES = 1024 * 512; // 512 KiB

export type GitFile = {
  path: string;
  /** porcelain XY code: e.g. " M", "M ", "??", "MM", "AD" */
  xy: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
  /** rename target → source for renamed files */
  origPath?: string;
};

export type GitStatus = {
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFile[];
};

/**
 * Validates that the given path is one we'll allow git ops on:
 * - exists and is a directory
 * - lives under one of the configured project roots, OR
 * - lives under our worktreeRoot, OR
 * - is the cwd / worktreePath / origPath of a currently-running tmux session
 *   (covers legacy sessions whose worktree predates a config change)
 */
export async function assertAllowedRepoPath(cwd: string): Promise<string> {
  const resolved = path.resolve(cwd);
  const wtRoot = path.resolve(worktreeRoot());
  const rel = path.relative(wtRoot, resolved);
  const insideWorktrees =
    rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
  const insideRoots = await isPathUnderRoots(resolved);

  let matchedSession = false;
  if (!insideWorktrees && !insideRoots) {
    try {
      const cfg = await loadConfig();
      const sessions = await listSessions(cfg.sessionPrefix);
      matchedSession = sessions.some((s) => {
        return (
          path.resolve(s.path || "") === resolved ||
          path.resolve(s.worktreePath || "") === resolved ||
          path.resolve(s.origPath || "") === resolved
        );
      });
    } catch {
      // If we can't reach tmux, fall through to the deny path.
    }
  }

  if (!insideWorktrees && !insideRoots && !matchedSession) {
    throw new Error("path is outside configured roots");
  }
  const st = await fs.stat(resolved).catch(() => null);
  if (!st || !st.isDirectory()) {
    throw new Error("path is not a directory");
  }
  return resolved;
}

export async function isGitRepo(cwd: string): Promise<boolean> {
  try {
    const { stdout } = await execFileP(
      "git",
      ["rev-parse", "--is-inside-work-tree"],
      { cwd },
    );
    return stdout.trim() === "true";
  } catch {
    return false;
  }
}

function parsePorcelainV2(stdout: string): GitFile[] {
  // We use `git status --porcelain=v1 -z` for simplicity. v1 line format:
  //   XY <path>
  //   XY <new>\0<old>  (for renames/copies, separated by NUL)
  // The caller pre-splits on NUL.
  const files: GitFile[] = [];
  const lines = stdout.split("\0").filter(Boolean);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const xy = line.slice(0, 2);
    const rest = line.slice(3); // skip "XY "
    let p = rest;
    let origPath: string | undefined;
    if (xy[0] === "R" || xy[0] === "C") {
      // rename/copy: next entry is the old path
      origPath = lines[++i] ?? undefined;
    }
    const staged = xy[0] !== " " && xy[0] !== "?";
    const unstaged = xy[1] !== " " && xy[1] !== "?";
    const untracked = xy === "??";
    files.push({ path: p, xy, staged, unstaged, untracked, origPath });
  }
  return files;
}

export async function gitStatus(cwd: string): Promise<GitStatus> {
  const empty: GitStatus = {
    branch: null,
    upstream: null,
    ahead: 0,
    behind: 0,
    files: [],
  };
  // Branch + ahead/behind
  let branch: string | null = null;
  let upstream: string | null = null;
  let ahead = 0;
  let behind = 0;
  try {
    const { stdout } = await execFileP(
      "git",
      ["rev-parse", "--abbrev-ref", "HEAD"],
      { cwd },
    );
    branch = stdout.trim() || null;
    if (branch === "HEAD") branch = null;
  } catch {
    return empty;
  }
  try {
    const { stdout } = await execFileP(
      "git",
      ["rev-parse", "--abbrev-ref", "@{upstream}"],
      { cwd },
    );
    upstream = stdout.trim() || null;
  } catch {
    upstream = null;
  }
  if (upstream) {
    try {
      const { stdout } = await execFileP(
        "git",
        ["rev-list", "--left-right", "--count", `${upstream}...HEAD`],
        { cwd },
      );
      const [b, a] = stdout.trim().split(/\s+/).map(Number);
      behind = Number.isFinite(b) ? b : 0;
      ahead = Number.isFinite(a) ? a : 0;
    } catch {
      // ignore
    }
  }
  // Files
  let files: GitFile[] = [];
  try {
    const { stdout } = await execFileP(
      "git",
      ["status", "--porcelain", "-z"],
      { cwd, maxBuffer: 1024 * 1024 * 4 },
    );
    files = parsePorcelainV2(stdout);
  } catch {
    files = [];
  }
  return { branch, upstream, ahead, behind, files };
}

export type GitDiffMode = "worktree" | "staged" | "head";

export async function gitDiff(opts: {
  cwd: string;
  file: string;
  mode: GitDiffMode;
}): Promise<{ diff: string; truncated: boolean; isUntracked: boolean }> {
  const args = ["-c", "color.ui=never", "diff", "--no-color"];
  if (opts.mode === "staged") args.push("--cached");
  if (opts.mode === "head") args.push("HEAD");
  args.push("--", opts.file);

  // First: handle untracked files (no diff target). Show contents as
  // a synthetic "+" diff so the UI looks consistent.
  let isUntracked = false;
  try {
    await execFileP("git", ["ls-files", "--error-unmatch", "--", opts.file], {
      cwd: opts.cwd,
    });
  } catch {
    // not tracked
    isUntracked = true;
  }

  if (isUntracked && opts.mode !== "staged") {
    try {
      const abs = path.resolve(opts.cwd, opts.file);
      const buf = await fs.readFile(abs);
      const truncated = buf.byteLength > MAX_DIFF_BYTES;
      const slice = truncated ? buf.subarray(0, MAX_DIFF_BYTES) : buf;
      const body = slice.toString("utf8");
      const synthetic =
        `diff --git a/${opts.file} b/${opts.file}\n` +
        `new file (untracked)\n` +
        `--- /dev/null\n` +
        `+++ b/${opts.file}\n` +
        body
          .split("\n")
          .map((l) => `+${l}`)
          .join("\n");
      return { diff: synthetic, truncated, isUntracked: true };
    } catch {
      return { diff: "", truncated: false, isUntracked: true };
    }
  }

  try {
    const { stdout } = await execFileP("git", args, {
      cwd: opts.cwd,
      maxBuffer: 1024 * 1024 * 8,
    });
    const buf = Buffer.from(stdout, "utf8");
    const truncated = buf.byteLength > MAX_DIFF_BYTES;
    const diff = truncated
      ? buf.subarray(0, MAX_DIFF_BYTES).toString("utf8") +
        "\n\n[…diff truncated…]\n"
      : stdout;
    return { diff, truncated, isUntracked };
  } catch (err) {
    return {
      diff: `[error] ${(err as Error).message}`,
      truncated: false,
      isUntracked,
    };
  }
}

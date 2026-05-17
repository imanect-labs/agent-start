import { execFile } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { worktreeRoot } from "./config";

const execFileP = promisify(execFile);

export type WorktreeCreated = {
  worktreePath: string;
  origPath: string;
  branch: string;
};

export async function isGitRepo(p: string): Promise<boolean> {
  try {
    await execFileP("git", ["-C", p, "rev-parse", "--git-dir"]);
    return true;
  } catch {
    return false;
  }
}

async function defaultBranch(repoPath: string): Promise<string> {
  try {
    const { stdout } = await execFileP("git", [
      "-C",
      repoPath,
      "symbolic-ref",
      "--short",
      "HEAD",
    ]);
    return stdout.trim() || "HEAD";
  } catch {
    return "HEAD";
  }
}

export function worktreePathFor(sessionName: string): string {
  return path.join(worktreeRoot(), sessionName);
}

export async function createWorktree(opts: {
  origPath: string;
  sessionName: string;
}): Promise<WorktreeCreated> {
  const { origPath, sessionName } = opts;
  if (!(await isGitRepo(origPath))) {
    throw new Error(`${origPath} is not a git repository`);
  }
  const wtPath = worktreePathFor(sessionName);
  await fs.mkdir(path.dirname(wtPath), { recursive: true });

  const base = await defaultBranch(origPath);
  const branch = `ccstart/${sessionName}`;
  await execFileP("git", [
    "-C",
    origPath,
    "worktree",
    "add",
    "-b",
    branch,
    wtPath,
    base,
  ]);
  return { worktreePath: wtPath, origPath, branch };
}

export async function removeWorktree(opts: {
  worktreePath: string;
  origPath?: string;
  removeBranch?: boolean;
}): Promise<void> {
  const { worktreePath, removeBranch } = opts;
  let origPath = opts.origPath;

  if (!origPath) {
    try {
      const { stdout } = await execFileP("git", [
        "-C",
        worktreePath,
        "rev-parse",
        "--git-common-dir",
      ]);
      origPath = path.dirname(stdout.trim());
    } catch {
      // worktree may be already broken
    }
  }

  let branch: string | undefined;
  if (removeBranch && origPath) {
    try {
      const { stdout } = await execFileP("git", [
        "-C",
        worktreePath,
        "symbolic-ref",
        "--short",
        "HEAD",
      ]);
      branch = stdout.trim();
    } catch {
      // detached
    }
  }

  if (origPath) {
    try {
      await execFileP("git", [
        "-C",
        origPath,
        "worktree",
        "remove",
        "--force",
        worktreePath,
      ]);
    } catch {
      // fall through to fs cleanup
    }
  }

  try {
    await fs.rm(worktreePath, { recursive: true, force: true });
  } catch {
    // ignore
  }

  if (origPath && branch && branch.startsWith("ccstart/")) {
    try {
      await execFileP("git", ["-C", origPath, "branch", "-D", branch]);
    } catch {
      // ignore — branch may not exist
    }
  }
}

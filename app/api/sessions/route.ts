import { NextResponse } from "next/server";
import path from "node:path";
import { loadConfig } from "@/lib/config";
import { isPathUnderRoots } from "@/lib/projects";
import { sessionName } from "@/lib/slug";
import {
  createSession,
  killSession,
  listSessions,
  setSessionOption,
} from "@/lib/tmux";
import {
  buildLaunchCommand,
  loadPreferences,
  sanitizeExtraArgs,
} from "@/lib/preferences";
import { createWorktree, isGitRepo, removeWorktree } from "@/lib/worktree";
import { markClaudeTrusted } from "@/lib/claude-trust";

export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const cfg = await loadConfig();
    const sessions = await listSessions(cfg.sessionPrefix);
    return NextResponse.json({ sessions });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

type StartBody = {
  projectPath: string;
  cli?: string;
  skipPermissions?: boolean;
  extraArgs?: string;
  createWorktree?: boolean;
};

export async function POST(req: Request) {
  let body: StartBody;
  try {
    body = (await req.json()) as StartBody;
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }
  if (!body.projectPath || typeof body.projectPath !== "string") {
    return NextResponse.json(
      { error: "projectPath is required" },
      { status: 400 },
    );
  }

  const resolved = path.resolve(body.projectPath);
  if (!(await isPathUnderRoots(resolved))) {
    return NextResponse.json(
      { error: "projectPath is outside configured roots" },
      { status: 400 },
    );
  }

  const cfg = await loadConfig();
  const prefs = await loadPreferences();

  const cliKey = body.cli ?? prefs.cli ?? cfg.defaultCli;
  const cliConf = cfg.clis[cliKey];
  if (!cliConf) {
    return NextResponse.json(
      { error: `unknown cli: ${cliKey}` },
      { status: 400 },
    );
  }

  const skipPermissions =
    typeof body.skipPermissions === "boolean"
      ? body.skipPermissions
      : prefs.skipPermissions;
  const extraArgsRaw =
    typeof body.extraArgs === "string" ? body.extraArgs : prefs.extraArgs;

  let extraArgs: string;
  try {
    extraArgs = sanitizeExtraArgs(extraArgsRaw);
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 400 },
    );
  }

  if (body.createWorktree && !(await isGitRepo(resolved))) {
    return NextResponse.json(
      { error: "createWorktree requested but project is not a git repository" },
      { status: 400 },
    );
  }

  const command = buildLaunchCommand(cliConf, { skipPermissions, extraArgs });
  const name = sessionName(cfg.sessionPrefix, path.basename(resolved));

  let cwd = resolved;
  let worktreePath: string | undefined;
  if (body.createWorktree) {
    try {
      const wt = await createWorktree({ origPath: resolved, sessionName: name });
      cwd = wt.worktreePath;
      worktreePath = wt.worktreePath;
    } catch (err) {
      return NextResponse.json(
        { error: `worktree creation failed: ${(err as Error).message}` },
        { status: 500 },
      );
    }
  }

  if (cliKey === "claude") {
    try {
      await markClaudeTrusted(cwd);
    } catch {
      // non-fatal: user will see the trust dialog at worst
    }
  }

  try {
    await createSession({ name, cwd, shell: cfg.shell, command });
    await setSessionOption(name, "@cli", cliKey);
    if (worktreePath) {
      await setSessionOption(name, "@worktree", worktreePath);
      await setSessionOption(name, "@origpath", resolved);
    }
    return NextResponse.json({
      name,
      command,
      cli: cliKey,
      cwd,
      worktreePath: worktreePath ?? null,
    });
  } catch (err) {
    // rollback worktree if session failed
    if (worktreePath) {
      try {
        await killSession(name);
      } catch {
        // ignore
      }
      await removeWorktree({
        worktreePath,
        origPath: resolved,
        removeBranch: true,
      });
    }
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

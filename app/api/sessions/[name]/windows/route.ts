import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import {
  getSession,
  isValidSessionName,
  listWindows,
  newWindow,
} from "@/lib/tmux";

export const dynamic = "force-dynamic";

export async function GET(
  _req: Request,
  ctx: { params: Promise<{ name: string }> },
) {
  const { name } = await ctx.params;
  const cfg = await loadConfig();
  if (!isValidSessionName(name) || !name.startsWith(cfg.sessionPrefix)) {
    return NextResponse.json(
      { error: "invalid session name" },
      { status: 400 },
    );
  }
  try {
    const windows = await listWindows(name);
    return NextResponse.json({ windows });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

export async function POST(
  _req: Request,
  ctx: { params: Promise<{ name: string }> },
) {
  const { name } = await ctx.params;
  const cfg = await loadConfig();
  if (!isValidSessionName(name) || !name.startsWith(cfg.sessionPrefix)) {
    return NextResponse.json(
      { error: "invalid session name" },
      { status: 400 },
    );
  }
  const session = await getSession(name);
  if (!session) {
    return NextResponse.json({ error: "session not found" }, { status: 404 });
  }
  // Default the new window cwd to the session's cwd so the user lands in the
  // same place as window 0 (which is usually a worktree). tmux will fall back
  // to the user's home if cwd is missing or invalid.
  const cwd = session.path || undefined;
  try {
    const index = await newWindow({
      session: name,
      cwd,
      shell: cfg.shell,
      // No command — leave the new window at an interactive shell prompt.
    });
    return NextResponse.json({ index });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

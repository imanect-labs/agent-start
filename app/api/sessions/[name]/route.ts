import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { getSession, isValidSessionName, killSession } from "@/lib/tmux";
import { removeWorktree } from "@/lib/worktree";

export const dynamic = "force-dynamic";

export async function DELETE(
  req: Request,
  ctx: { params: Promise<{ name: string }> },
) {
  const { name } = await ctx.params;
  const cfg = await loadConfig();
  if (!isValidSessionName(name) || !name.startsWith(cfg.sessionPrefix)) {
    return NextResponse.json({ error: "invalid session name" }, { status: 400 });
  }

  const url = new URL(req.url);
  const deleteWorktree = url.searchParams.get("deleteWorktree") === "1";

  const session = await getSession(name);

  try {
    await killSession(name);
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }

  let worktreeRemoved = false;
  if (deleteWorktree && session?.worktreePath) {
    try {
      await removeWorktree({
        worktreePath: session.worktreePath,
        origPath: session.origPath || undefined,
        removeBranch: true,
      });
      worktreeRemoved = true;
    } catch (err) {
      return NextResponse.json(
        {
          ok: true,
          worktreeRemoved: false,
          worktreeError: (err as Error).message,
        },
        { status: 200 },
      );
    }
  }

  return NextResponse.json({ ok: true, worktreeRemoved });
}

import { NextResponse } from "next/server";
import {
  assertAllowedRepoPath,
  gitDiff,
  isGitRepo,
  type GitDiffMode,
} from "@/lib/git";

export const dynamic = "force-dynamic";

export async function GET(req: Request) {
  const url = new URL(req.url);
  const cwd = url.searchParams.get("path");
  const file = url.searchParams.get("file");
  const rawMode = url.searchParams.get("mode") ?? "worktree";
  if (!cwd) {
    return NextResponse.json({ error: "path is required" }, { status: 400 });
  }
  if (!file) {
    return NextResponse.json({ error: "file is required" }, { status: 400 });
  }
  // Reject absolute paths / parent traversal — file must be a repo-relative
  // path. The git command itself enforces this too but we fail fast.
  if (file.startsWith("/") || file.split("/").some((seg) => seg === "..")) {
    return NextResponse.json({ error: "invalid file path" }, { status: 400 });
  }
  const mode = (
    rawMode === "staged" || rawMode === "head" ? rawMode : "worktree"
  ) as GitDiffMode;

  let resolved: string;
  try {
    resolved = await assertAllowedRepoPath(cwd);
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 400 },
    );
  }
  if (!(await isGitRepo(resolved))) {
    return NextResponse.json({ error: "not a git repo" }, { status: 400 });
  }
  try {
    const res = await gitDiff({ cwd: resolved, file, mode });
    return NextResponse.json(res);
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

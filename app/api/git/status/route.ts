import { NextResponse } from "next/server";
import { assertAllowedRepoPath, gitStatus, isGitRepo } from "@/lib/git";

export const dynamic = "force-dynamic";

export async function GET(req: Request) {
  const url = new URL(req.url);
  const cwd = url.searchParams.get("path");
  if (!cwd) {
    return NextResponse.json({ error: "path is required" }, { status: 400 });
  }
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
    return NextResponse.json({ isGit: false }, { status: 200 });
  }
  try {
    const status = await gitStatus(resolved);
    return NextResponse.json({ isGit: true, ...status });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

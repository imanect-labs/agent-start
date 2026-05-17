import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { capturePane, isValidSessionName } from "@/lib/tmux";

export const dynamic = "force-dynamic";

export async function GET(
  _req: Request,
  ctx: { params: Promise<{ name: string }> },
) {
  const { name } = await ctx.params;
  const cfg = await loadConfig();
  if (!isValidSessionName(name) || !name.startsWith(cfg.sessionPrefix)) {
    return NextResponse.json({ error: "invalid session name" }, { status: 400 });
  }
  try {
    const output = await capturePane(name);
    return NextResponse.json({ output });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

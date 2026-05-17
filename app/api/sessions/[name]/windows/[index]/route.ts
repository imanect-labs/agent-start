import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { isValidSessionName, killWindow } from "@/lib/tmux";

export const dynamic = "force-dynamic";

export async function DELETE(
  _req: Request,
  ctx: { params: Promise<{ name: string; index: string }> },
) {
  const { name, index } = await ctx.params;
  const cfg = await loadConfig();
  if (!isValidSessionName(name) || !name.startsWith(cfg.sessionPrefix)) {
    return NextResponse.json(
      { error: "invalid session name" },
      { status: 400 },
    );
  }
  const n = Number(index);
  if (!Number.isInteger(n) || n < 0 || n > 9999) {
    return NextResponse.json(
      { error: "invalid window index" },
      { status: 400 },
    );
  }
  try {
    await killWindow(name, n);
    return NextResponse.json({ ok: true });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

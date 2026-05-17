import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";

export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const cfg = await loadConfig();
    const clis = Object.entries(cfg.clis).map(([key, c]) => ({
      key,
      label: c.label ?? key,
      command: c.command,
      hasSkipFlag: !!c.skipPermissionsFlag,
      skipFlag: c.skipPermissionsFlag ?? "",
    }));
    return NextResponse.json({
      clis,
      defaultCli: cfg.defaultCli,
      sessionPrefix: cfg.sessionPrefix,
    });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

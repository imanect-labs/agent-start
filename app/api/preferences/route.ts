import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import {
  DEFAULT_PREFERENCES,
  loadPreferences,
  sanitizeExtraArgs,
  savePreferences,
  type Preferences,
} from "@/lib/preferences";

export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const prefs = await loadPreferences();
    return NextResponse.json({ preferences: prefs });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

export async function PUT(req: Request) {
  let body: Partial<Preferences>;
  try {
    body = (await req.json()) as Partial<Preferences>;
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }
  try {
    const cfg = await loadConfig();
    const current = await loadPreferences();
    const next: Preferences = {
      ...DEFAULT_PREFERENCES,
      ...current,
    };
    if (typeof body.cli === "string") {
      if (!cfg.clis[body.cli]) {
        return NextResponse.json(
          { error: `unknown cli: ${body.cli}` },
          { status: 400 },
        );
      }
      next.cli = body.cli;
    }
    if (typeof body.skipPermissions === "boolean") {
      next.skipPermissions = body.skipPermissions;
    }
    if (typeof body.extraArgs === "string") {
      next.extraArgs = sanitizeExtraArgs(body.extraArgs);
    }
    await savePreferences(next);
    return NextResponse.json({ preferences: next });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 400 },
    );
  }
}

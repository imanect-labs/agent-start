import { NextResponse } from "next/server";
import os from "node:os";
import path from "node:path";
import { loadConfig, worktreeRoot, preferencesPath } from "@/lib/config";

export const dynamic = "force-dynamic";

function configPathPublic(): string {
  if (process.env.AGENT_START_CONFIG) return process.env.AGENT_START_CONFIG;
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
  return path.join(xdg, "agent-start", "config.json");
}

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
      roots: cfg.roots,
      shell: cfg.shell,
      showHidden: cfg.showHidden,
      gitOnly: cfg.gitOnly,
      paths: {
        config: configPathPublic(),
        preferences: preferencesPath(),
        worktreeRoot: worktreeRoot(),
      },
    });
  } catch (err) {
    return NextResponse.json(
      { error: (err as Error).message },
      { status: 500 },
    );
  }
}

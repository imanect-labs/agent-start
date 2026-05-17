import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const CLAUDE_JSON = path.join(os.homedir(), ".claude.json");

type ProjectEntry = {
  hasTrustDialogAccepted?: boolean;
  [k: string]: unknown;
};
type ClaudeConfig = {
  projects?: Record<string, ProjectEntry>;
  [k: string]: unknown;
};

/** Pre-accept the Claude Code workspace trust dialog for the given directory. */
export async function markClaudeTrusted(dir: string): Promise<void> {
  const target = path.resolve(dir);
  let cfg: ClaudeConfig = {};
  try {
    const raw = await fs.readFile(CLAUDE_JSON, "utf8");
    cfg = JSON.parse(raw) as ClaudeConfig;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") throw err;
  }
  const projects = cfg.projects ?? {};
  const existing = projects[target] ?? {};
  if (existing.hasTrustDialogAccepted === true) return;
  projects[target] = {
    allowedTools: [],
    mcpContextUris: [],
    mcpServers: {},
    enabledMcpjsonServers: [],
    disabledMcpjsonServers: [],
    ...existing,
    hasTrustDialogAccepted: true,
  };
  cfg.projects = projects;
  // write atomically
  const tmp = `${CLAUDE_JSON}.ccstart.tmp.${process.pid}`;
  await fs.writeFile(tmp, JSON.stringify(cfg, null, 2), { mode: 0o600 });
  await fs.rename(tmp, CLAUDE_JSON);
}

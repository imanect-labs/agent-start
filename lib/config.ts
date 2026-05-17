import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

export type CliConfig = {
  command: string;
  skipPermissionsFlag?: string;
  label?: string;
};

export type Config = {
  roots: string[];
  sessionPrefix: string;
  shell: string;
  showHidden: boolean;
  gitOnly: boolean;
  clis: Record<string, CliConfig>;
  defaultCli: string;
};

const DEFAULT_CONFIG: Config = {
  roots: [path.join(os.homedir(), "dev")],
  sessionPrefix: "cc-",
  shell: "/bin/bash",
  showHidden: false,
  gitOnly: false,
  clis: {
    claude: {
      command: "claude",
      skipPermissionsFlag: "--dangerously-skip-permissions",
      label: "Claude Code",
    },
    codex: {
      command: "codex",
      skipPermissionsFlag: "--full-auto",
      label: "Codex CLI",
    },
    shell: {
      // empty command → start an interactive login shell only (no CLI auto-run)
      command: "",
      label: "Terminal",
    },
  },
  defaultCli: "claude",
};

function configPath(): string {
  if (process.env.CCSTART_CONFIG) return process.env.CCSTART_CONFIG;
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
  return path.join(xdg, "ccstart", "config.json");
}

export function preferencesPath(): string {
  if (process.env.CCSTART_PREFS) return process.env.CCSTART_PREFS;
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
  return path.join(xdg, "ccstart", "preferences.json");
}

export function worktreeRoot(): string {
  if (process.env.CCSTART_WORKTREE_ROOT) return process.env.CCSTART_WORKTREE_ROOT;
  const xdg = process.env.XDG_CACHE_HOME || path.join(os.homedir(), ".cache");
  return path.join(xdg, "ccstart", "worktrees");
}

let cached: Config | null = null;

function migrate(raw: Record<string, unknown>): Partial<Config> {
  const out: Partial<Config> = { ...raw } as Partial<Config>;
  // legacy: top-level `claudeCommand` → clis.claude.command
  if (typeof raw.claudeCommand === "string") {
    const clis = (raw.clis as Record<string, CliConfig>) || {
      ...DEFAULT_CONFIG.clis,
    };
    clis.claude = {
      ...DEFAULT_CONFIG.clis.claude,
      ...clis.claude,
      command: raw.claudeCommand as string,
    };
    out.clis = clis;
    delete (out as Record<string, unknown>).claudeCommand;
  }
  return out;
}

export async function loadConfig(): Promise<Config> {
  if (cached) return cached;
  const p = configPath();
  try {
    const raw = await fs.readFile(p, "utf8");
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const migrated = migrate(parsed);
    cached = {
      ...DEFAULT_CONFIG,
      ...migrated,
      clis: { ...DEFAULT_CONFIG.clis, ...(migrated.clis ?? {}) },
    };
    // Persist migration so user sees it
    if ((parsed as Record<string, unknown>).claudeCommand !== undefined) {
      await fs.writeFile(p, JSON.stringify(cached, null, 2));
    }
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") {
      await fs.mkdir(path.dirname(p), { recursive: true });
      await fs.writeFile(p, JSON.stringify(DEFAULT_CONFIG, null, 2));
      cached = DEFAULT_CONFIG;
    } else {
      throw err;
    }
  }
  return cached;
}

export function clearConfigCache(): void {
  cached = null;
}

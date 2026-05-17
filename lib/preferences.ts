import { promises as fs } from "node:fs";
import path from "node:path";
import { CliConfig, loadConfig, preferencesPath } from "./config";

export type Preferences = {
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
};

export const DEFAULT_PREFERENCES: Preferences = {
  cli: "claude",
  skipPermissions: true,
  extraArgs: "",
};

function migrate(raw: Record<string, unknown>): Partial<Preferences> {
  const out: Partial<Preferences> = { ...raw } as Partial<Preferences>;
  // legacy: dangerouslySkipPermissions → skipPermissions
  if (
    out.skipPermissions === undefined &&
    typeof raw.dangerouslySkipPermissions === "boolean"
  ) {
    out.skipPermissions = raw.dangerouslySkipPermissions as boolean;
    delete (out as Record<string, unknown>).dangerouslySkipPermissions;
  }
  return out;
}

export async function loadPreferences(): Promise<Preferences> {
  const cfg = await loadConfig();
  const defaults: Preferences = {
    ...DEFAULT_PREFERENCES,
    cli: cfg.defaultCli || DEFAULT_PREFERENCES.cli,
  };
  const p = preferencesPath();
  try {
    const raw = await fs.readFile(p, "utf8");
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const migrated = migrate(parsed);
    return { ...defaults, ...migrated };
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") {
      return defaults;
    }
    throw err;
  }
}

export async function savePreferences(prefs: Preferences): Promise<void> {
  const p = preferencesPath();
  await fs.mkdir(path.dirname(p), { recursive: true });
  await fs.writeFile(p, JSON.stringify(prefs, null, 2));
}

const SHELL_SAFE = /^[A-Za-z0-9_\-./= ]*$/;

export function sanitizeExtraArgs(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "";
  if (!SHELL_SAFE.test(trimmed)) {
    throw new Error(
      "extraArgs contains unsupported characters. Allowed: letters, digits, space, _ - . / =",
    );
  }
  return trimmed;
}

export function buildLaunchCommand(
  cli: CliConfig,
  opts: { skipPermissions: boolean; extraArgs: string },
): string {
  const parts: string[] = [cli.command];
  if (opts.skipPermissions && cli.skipPermissionsFlag) {
    parts.push(cli.skipPermissionsFlag);
  }
  const extra = sanitizeExtraArgs(opts.extraArgs || "");
  if (extra) parts.push(extra);
  return parts.join(" ");
}

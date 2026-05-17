import { promises as fs } from "node:fs";
import path from "node:path";
import { loadConfig } from "./config";

export type Project = {
  name: string;
  path: string;
  root: string;
  mtimeMs: number;
  isGit: boolean;
};

async function safeReaddir(dir: string) {
  try {
    return await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return [];
  }
}

export async function listProjects(): Promise<Project[]> {
  const cfg = await loadConfig();
  const out: Project[] = [];

  for (const root of cfg.roots) {
    const resolvedRoot = path.resolve(root);
    const entries = await safeReaddir(resolvedRoot);
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (!cfg.showHidden && entry.name.startsWith(".")) continue;
      const full = path.join(resolvedRoot, entry.name);
      let isGit = false;
      try {
        const st = await fs.stat(path.join(full, ".git"));
        isGit = st.isDirectory() || st.isFile();
      } catch {
        isGit = false;
      }
      if (cfg.gitOnly && !isGit) continue;
      let mtimeMs = 0;
      try {
        const st = await fs.stat(full);
        mtimeMs = st.mtimeMs;
      } catch {
        // ignore
      }
      out.push({
        name: entry.name,
        path: full,
        root: resolvedRoot,
        mtimeMs,
        isGit,
      });
    }
  }
  out.sort((a, b) => b.mtimeMs - a.mtimeMs);
  return out;
}

export async function isPathUnderRoots(targetPath: string): Promise<boolean> {
  const cfg = await loadConfig();
  const resolved = path.resolve(targetPath);
  return cfg.roots.some((r) => {
    const root = path.resolve(r);
    const rel = path.relative(root, resolved);
    return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
  });
}

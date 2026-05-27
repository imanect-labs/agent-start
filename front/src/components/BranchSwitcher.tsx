import useSWR from "swr";
import { useEffect, useRef, useState } from "react";
import { IconBranch, IconCheck, IconPlus, IconRefresh, IconTrash, IconX } from "@/components/icons";

type Branch = {
  name: string;
  current: boolean;
  upstream?: string | null;
  ahead: number;
  behind: number;
  isRemote: boolean;
};

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    const json = await r.json();
    if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
    return json;
  });

async function post(url: string, body: unknown) {
  const r = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
  return json;
}

/**
 * Branch dropdown + remote sync (fetch/pull/push) for the current repo.
 * After any mutation it refreshes its own branch list and calls
 * `onRepoChanged` so the surrounding FilesView status refreshes too.
 */
export function BranchSwitcher({
  cwd,
  onRepoChanged,
}: {
  cwd: string;
  onRepoChanged?: () => void;
}) {
  const key = cwd ? `/api/git/branches?path=${encodeURIComponent(cwd)}` : null;
  const { data, mutate } = useSWR<{ branches: Branch[] }>(key, fetcher);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const branches = data?.branches ?? [];
  const current = branches.find((b) => b.current);
  const locals = branches.filter((b) => !b.isRemote);

  async function run(fn: () => Promise<unknown>) {
    setBusy(true);
    try {
      await fn();
      await mutate();
      onRepoChanged?.();
    } catch (e) {
      alert((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  const checkout = (name: string) =>
    run(async () => {
      await post("/api/git/checkout", { path: cwd, name });
      setOpen(false);
    });
  const del = (name: string) => {
    if (!window.confirm(`ブランチ ${name} を削除しますか？`)) return;
    return run(async () => {
      try {
        await post("/api/git/branches/delete", { path: cwd, name, force: false });
      } catch (e) {
        // `git branch -d` only refuses *unmerged* branches; for any other
        // failure (auth, not found, …) surface the original error.
        const unmerged = /not fully merged|not merged/i.test((e as Error).message);
        if (unmerged && window.confirm(`${name} は未マージです。強制削除しますか？`)) {
          await post("/api/git/branches/delete", { path: cwd, name, force: true });
        } else {
          throw e;
        }
      }
    });
  };
  const create = () => {
    const name = newName.trim();
    if (!name) return;
    return run(async () => {
      await post("/api/git/branches", { path: cwd, name, checkout: true });
      setNewName("");
      setOpen(false);
    });
  };
  const sync = (endpoint: "fetch" | "pull" | "push") =>
    run(async () => {
      // push/pull need a concrete branch; refuse in detached HEAD rather
      // than send an ambiguous request (and never set upstream then).
      if (endpoint !== "fetch" && !current?.name) {
        throw new Error("現在のブランチが特定できません（detached HEAD）");
      }
      const setUpstream = endpoint === "push" && !!current?.name && !current.upstream;
      await post(`/api/git/${endpoint}`, {
        path: cwd,
        remote: "origin",
        branch: endpoint === "fetch" ? undefined : current?.name,
        setUpstream,
      });
    });

  return (
    <div className="flex items-center gap-1.5">
      <div ref={ref} className="relative">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          disabled={busy}
          className="inline-flex items-center gap-1.5 max-w-[200px] rounded-md border border-line bg-surface px-2 py-1 text-sm hover:bg-surface-muted disabled:opacity-50"
        >
          <IconBranch className="w-3.5 h-3.5 text-fg-faint shrink-0" />
          <span className="font-mono truncate">{current?.name ?? "(detached)"}</span>
          {current && (current.ahead > 0 || current.behind > 0) && (
            <span className="text-[10px] text-fg-faint shrink-0">
              {current.ahead ? `↑${current.ahead}` : ""}
              {current.behind ? `↓${current.behind}` : ""}
            </span>
          )}
        </button>
        {open && (
          <div className="absolute left-0 top-full mt-1 z-30 w-64 bg-surface-elev border border-line rounded-md shadow-lg py-1">
            <div className="max-h-60 overflow-y-auto scroll-thin">
              {locals.length === 0 && (
                <div className="px-3 py-2 text-xs text-fg-subtle">ブランチがありません</div>
              )}
              {locals.map((b) => (
                <div
                  key={b.name}
                  className="group/b flex items-center gap-1.5 px-2 py-1 hover:bg-surface-muted"
                >
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => checkout(b.name)}
                    className="flex-1 min-w-0 text-left flex items-center gap-1.5 disabled:opacity-50"
                  >
                    <span className="w-3 shrink-0 text-accent">
                      {b.current ? <IconCheck className="w-3 h-3" /> : null}
                    </span>
                    <span className="font-mono text-[12px] truncate">{b.name}</span>
                  </button>
                  {!b.current && (
                    <button
                      type="button"
                      title="削除"
                      disabled={busy}
                      onClick={() => del(b.name)}
                      className="p-0.5 rounded text-danger opacity-0 group-hover/b:opacity-100 hover:bg-surface disabled:opacity-50"
                    >
                      <IconTrash className="w-3 h-3" />
                    </button>
                  )}
                </div>
              ))}
            </div>
            <div className="border-t border-line mt-1 pt-1 px-2 pb-1">
              <div className="flex items-center gap-1">
                <input
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") create();
                  }}
                  placeholder="新しいブランチ名"
                  className="flex-1 min-w-0 rounded border border-line bg-surface px-2 py-1 text-[12px] font-mono outline-none focus-visible:ring-2 focus-visible:ring-ring/20"
                />
                <button
                  type="button"
                  title="作成して切替"
                  disabled={busy || newName.trim().length === 0}
                  onClick={create}
                  className="p-1 rounded border border-line bg-surface hover:bg-surface-muted disabled:opacity-50"
                >
                  <IconPlus className="w-3 h-3" />
                </button>
                <button
                  type="button"
                  title="閉じる"
                  onClick={() => setOpen(false)}
                  className="p-1 rounded text-fg-faint hover:bg-surface-muted"
                >
                  <IconX className="w-3 h-3" />
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      <SyncButton label="Fetch" busy={busy} onClick={() => sync("fetch")} />
      <SyncButton label="Pull" busy={busy} onClick={() => sync("pull")} />
      <SyncButton label="Push" busy={busy} onClick={() => sync("push")} />
    </div>
  );
}

function SyncButton({
  label,
  busy,
  onClick,
}: {
  label: string;
  busy: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={busy}
      onClick={onClick}
      className="inline-flex items-center gap-1 rounded-md border border-line bg-surface px-2 py-1 text-[11px] text-fg-muted hover:bg-surface-muted disabled:opacity-50"
    >
      <IconRefresh className={`w-3 h-3 ${busy ? "animate-spin" : ""}`} />
      {label}
    </button>
  );
}

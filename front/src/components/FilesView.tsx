import useSWR from "swr";
import { useMemo, useState } from "react";
import { Button, EmptyState, ErrorState, Skeleton, SkeletonRows } from "@/components/ui";
import { IconCheck, IconPlus, IconRefresh, IconTrash, IconX } from "@/components/icons";
import { BranchSwitcher } from "@/components/BranchSwitcher";
import type { DiffMode } from "@/components/tab-types";

type GitFile = {
  path: string;
  xy: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
  origPath?: string;
};

type GitStatus = {
  isGit: boolean;
  branch?: string | null;
  upstream?: string | null;
  ahead?: number;
  behind?: number;
  files?: GitFile[];
  error?: string;
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

function fileStatusLabel(f: GitFile): { label: string; tone: string } {
  if (f.untracked) return { label: "??", tone: "text-add" };
  const x = f.xy[0];
  const y = f.xy[1];
  // Prefer the most informative single-char status.
  if (x === "R" || y === "R") return { label: "R", tone: "text-accent-subtle" };
  if (x === "A" || y === "A") return { label: "A", tone: "text-add" };
  if (x === "D" || y === "D") return { label: "D", tone: "text-del" };
  if (x === "M" || y === "M") return { label: "M", tone: "text-warn" };
  return { label: f.xy.trim() || "?", tone: "text-fg-faint" };
}

export function FilesView({
  cwd,
  fullWidth = false,
  onOpenDiff,
}: {
  cwd: string;
  fullWidth?: boolean;
  onOpenDiff?: (file: string, mode: DiffMode) => void;
}) {
  const key = cwd ? `/api/git/status?path=${encodeURIComponent(cwd)}` : null;
  const { data, error, isLoading, mutate } = useSWR<GitStatus>(key, fetcher, {
    refreshInterval: 8000,
  });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  const files = data?.files ?? [];
  const grouped = useMemo(() => {
    const staged: GitFile[] = [];
    const unstaged: GitFile[] = [];
    const untracked: GitFile[] = [];
    for (const f of files) {
      if (f.untracked) untracked.push(f);
      else {
        if (f.staged) staged.push(f);
        if (f.unstaged) unstaged.push(f);
      }
    }
    return { staged, unstaged, untracked };
  }, [files]);

  if (!cwd) {
    return <Empty>セッションの作業ディレクトリが特定できません</Empty>;
  }
  if (isLoading && !data) {
    // Reserve roughly the same area the real header + list would
    // occupy so the pane doesn't pop in height when data arrives.
    return (
      <div className="p-3 space-y-3 min-h-[260px]">
        <Skeleton style={{ height: 20, width: "55%" }} />
        <Skeleton style={{ height: 12, width: "35%" }} />
        <SkeletonRows n={4} rowHeight={26} className="mt-2" />
      </div>
    );
  }
  if (error) {
    return (
      <ErrorState
        compact
        title="git ステータスの取得に失敗しました"
        description={(error as Error).message}
        onRetry={() => mutate()}
      />
    );
  }
  if (!data?.isGit) {
    return <Empty>このセッションは git リポジトリではありません</Empty>;
  }

  // Run a mutation, then refresh status immediately rather than waiting
  // for the 8s poll. Surface failures inline via alert (no toast lib).
  async function run(fn: () => Promise<unknown>) {
    setBusy(true);
    try {
      await fn();
      await mutate();
    } catch (e) {
      alert((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  const stage = (paths: string[]) => run(() => post("/api/git/stage", { path: cwd, files: paths }));
  const unstage = (paths: string[]) =>
    run(() => post("/api/git/unstage", { path: cwd, files: paths }));
  const discard = (paths: string[]) => {
    const label = paths.length === 1 ? paths[0] : `${paths.length} 件のファイル`;
    if (!window.confirm(`${label} の変更を破棄します。元に戻せません。よろしいですか？`)) return;
    return run(() => post("/api/git/discard", { path: cwd, files: paths }));
  };
  const doCommit = () =>
    run(async () => {
      await post("/api/git/commit", { path: cwd, message });
      setMessage("");
    });

  return (
    <div className={fullWidth ? "p-4 space-y-3" : "p-3 space-y-3"}>
      <div className="flex items-start gap-2 flex-wrap">
        <div className="flex-1 min-w-0">
          <BranchSwitcher cwd={cwd} onRepoChanged={() => mutate()} />
          <div className="text-2xs text-fg-faint mt-1 flex gap-2">
            <span>{files.length} files</span>
            {data.upstream && <span className="truncate">↑ {data.upstream}</span>}
          </div>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => mutate()}
          leftIcon={<IconRefresh className="w-3 h-3" />}
        >
          更新
        </Button>
      </div>

      {files.length === 0 ? (
        <EmptyState compact icon={<IconCheck />} title="変更はありません" />
      ) : (
        <div className="space-y-3">
          {grouped.staged.length > 0 && (
            <FileGroup
              title="staged"
              mode="staged"
              files={grouped.staged}
              busy={busy}
              onSelect={(f, m) => onOpenDiff?.(f, m)}
              actions={[
                {
                  key: "unstage",
                  label: "Unstage",
                  icon: <IconX className="w-3 h-3" />,
                  run: unstage,
                },
              ]}
            />
          )}
          {grouped.staged.length > 0 && (
            <CommitBox
              message={message}
              onMessage={setMessage}
              disabled={busy}
              onCommit={doCommit}
            />
          )}
          {grouped.unstaged.length > 0 && (
            <FileGroup
              title="changes"
              mode="worktree"
              files={grouped.unstaged}
              busy={busy}
              onSelect={(f, m) => onOpenDiff?.(f, m)}
              actions={[
                {
                  key: "stage",
                  label: "Stage",
                  icon: <IconPlus className="w-3 h-3" />,
                  run: stage,
                },
                {
                  key: "discard",
                  label: "Discard",
                  icon: <IconTrash className="w-3 h-3" />,
                  danger: true,
                  run: discard,
                },
              ]}
            />
          )}
          {grouped.untracked.length > 0 && (
            <FileGroup
              title="untracked"
              mode="worktree"
              files={grouped.untracked}
              busy={busy}
              onSelect={(f, m) => onOpenDiff?.(f, m)}
              actions={[
                {
                  key: "stage",
                  label: "Stage",
                  icon: <IconPlus className="w-3 h-3" />,
                  run: stage,
                },
                {
                  key: "discard",
                  label: "Delete",
                  icon: <IconTrash className="w-3 h-3" />,
                  danger: true,
                  run: discard,
                },
              ]}
            />
          )}
        </div>
      )}
    </div>
  );
}

type GroupAction = {
  key: string;
  label: string;
  icon: React.ReactNode;
  danger?: boolean;
  run: (paths: string[]) => void;
};

function CommitBox({
  message,
  onMessage,
  disabled,
  onCommit,
}: {
  message: string;
  onMessage: (v: string) => void;
  disabled: boolean;
  onCommit: () => void;
}) {
  return (
    <div className="space-y-2">
      <textarea
        value={message}
        onChange={(e) => onMessage(e.target.value)}
        disabled={disabled}
        placeholder="コミットメッセージ"
        rows={2}
        className="w-full resize-y rounded-md border border-line bg-surface px-2.5 py-1.5 text-xs font-mono outline-none focus-visible:ring-2 focus-visible:ring-ring/20 disabled:opacity-50"
      />
      <Button
        variant="primary"
        size="sm"
        disabled={disabled || message.trim().length === 0}
        onClick={onCommit}
        leftIcon={<IconCheck className="w-3 h-3" />}
      >
        コミット
      </Button>
    </div>
  );
}

function FileGroup({
  title,
  mode,
  files,
  busy,
  actions,
  onSelect,
}: {
  title: string;
  mode: DiffMode;
  files: GitFile[];
  busy: boolean;
  actions: GroupAction[];
  onSelect: (file: string, mode: DiffMode) => void;
}) {
  const allPaths = files.map((f) => f.path);
  return (
    <div>
      <div className="text-2xs uppercase tracking-wider text-fg-faint font-medium mb-1.5 flex items-center justify-between gap-2">
        <span className="flex items-center gap-1.5">
          <span>{title}</span>
          <span className="tabular-nums">{files.length}</span>
        </span>
        <span className="flex items-center gap-1">
          {actions.map((a) => (
            <button
              key={a.key}
              type="button"
              disabled={busy}
              onClick={() => a.run(allPaths)}
              className={[
                "text-2xs px-1.5 py-0.5 rounded border border-line bg-surface hover:bg-surface-muted disabled:opacity-50",
                a.danger ? "text-danger hover:border-danger/50" : "text-fg-muted",
              ].join(" ")}
            >
              {a.label} all
            </button>
          ))}
        </span>
      </div>
      <ul className="border border-line rounded-md overflow-hidden">
        {files.map((f) => {
          const status = fileStatusLabel(f);
          return (
            <li
              key={`${mode}:${f.path}`}
              className="group/row flex items-stretch border-b border-line last:border-b-0 bg-surface hover:bg-surface-muted"
            >
              <button
                type="button"
                onClick={() => onSelect(f.path, mode)}
                className="flex-1 min-w-0 text-left px-2.5 py-1.5 flex items-center gap-2 text-fg-muted"
              >
                <span className={`shrink-0 font-mono text-2xs w-4 text-center ${status.tone}`}>
                  {status.label}
                </span>
                <span className="font-mono text-xs truncate flex-1 min-w-0">
                  {f.origPath ? (
                    <>
                      <span className="text-fg-faint">{f.origPath} → </span>
                      {f.path}
                    </>
                  ) : (
                    f.path
                  )}
                </span>
              </button>
              <span className="flex items-center gap-0.5 pr-1.5 opacity-0 group-hover/row:opacity-100 focus-within:opacity-100">
                {actions.map((a) => (
                  <button
                    key={a.key}
                    type="button"
                    title={a.label}
                    disabled={busy}
                    onClick={() => a.run([f.path])}
                    className={[
                      "p-1 rounded hover:bg-surface-muted disabled:opacity-50",
                      a.danger ? "text-danger" : "text-fg-faint hover:text-fg",
                    ].join(" ")}
                  >
                    {a.icon}
                  </button>
                ))}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-[120px] flex items-center justify-center text-center text-xs text-fg-subtle py-6 px-4">
      {children}
    </div>
  );
}

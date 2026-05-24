import useSWR from "swr";
import { useMemo } from "react";
import { Button, Spinner } from "@/components/ui";
import { IconBranch, IconRefresh } from "@/components/icons";
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

function fileStatusLabel(f: GitFile): { label: string; tone: string } {
  if (f.untracked) return { label: "??", tone: "text-emerald-500" };
  const x = f.xy[0];
  const y = f.xy[1];
  // Prefer the most informative single-char status.
  if (x === "R" || y === "R") return { label: "R", tone: "text-blue-500" };
  if (x === "A" || y === "A") return { label: "A", tone: "text-emerald-500" };
  if (x === "D" || y === "D") return { label: "D", tone: "text-red-500" };
  if (x === "M" || y === "M") return { label: "M", tone: "text-amber-500" };
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

  if (!cwd) {
    return <Empty>セッションの作業ディレクトリが特定できません</Empty>;
  }
  if (isLoading && !data) {
    return (
      <div className="flex justify-center py-8">
        <Spinner size="sm" />
      </div>
    );
  }
  if (error) {
    return <Empty>取得に失敗: {(error as Error).message}</Empty>;
  }
  if (!data?.isGit) {
    return <Empty>このセッションは git リポジトリではありません</Empty>;
  }

  const files = data.files ?? [];

  return (
    <div className={fullWidth ? "p-4 space-y-3" : "p-3 space-y-3"}>
      <div className="flex items-center gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 text-sm">
            <IconBranch className="w-3.5 h-3.5 text-fg-faint shrink-0" />
            <span className="font-mono truncate">{data.branch ?? "(detached)"}</span>
            {data.upstream && (
              <span className="text-fg-faint text-[11px] truncate">↑ {data.upstream}</span>
            )}
          </div>
          <div className="text-[11px] text-fg-faint mt-0.5 flex gap-2">
            <span>{files.length} files</span>
            {(data.ahead || data.behind) && (
              <span>
                {data.ahead ? `+${data.ahead}` : ""}
                {data.behind ? ` -${data.behind}` : ""}
              </span>
            )}
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
        <Empty>変更はありません</Empty>
      ) : (
        <FileList files={files} onSelect={(f, m) => onOpenDiff?.(f, m)} />
      )}
    </div>
  );
}

function FileList({
  files,
  onSelect,
}: {
  files: GitFile[];
  onSelect: (file: string, mode: DiffMode) => void;
}) {
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

  return (
    <div className="space-y-3">
      {grouped.staged.length > 0 && (
        <FileGroup title="staged" mode="staged" files={grouped.staged} onSelect={onSelect} />
      )}
      {grouped.unstaged.length > 0 && (
        <FileGroup title="changes" mode="worktree" files={grouped.unstaged} onSelect={onSelect} />
      )}
      {grouped.untracked.length > 0 && (
        <FileGroup
          title="untracked"
          mode="worktree"
          files={grouped.untracked}
          onSelect={onSelect}
        />
      )}
    </div>
  );
}

function FileGroup({
  title,
  mode,
  files,
  onSelect,
}: {
  title: string;
  mode: DiffMode;
  files: GitFile[];
  onSelect: (file: string, mode: DiffMode) => void;
}) {
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wider text-fg-faint font-medium mb-1.5 flex items-center justify-between">
        <span>{title}</span>
        <span className="tabular-nums">{files.length}</span>
      </div>
      <ul className="border border-line rounded-md overflow-hidden">
        {files.map((f) => {
          const status = fileStatusLabel(f);
          return (
            <li key={`${mode}:${f.path}`}>
              <button
                type="button"
                onClick={() => onSelect(f.path, mode)}
                className={[
                  "w-full text-left px-2.5 py-1.5 flex items-center gap-2",
                  "border-b border-line last:border-b-0",
                  "bg-surface hover:bg-surface-muted text-fg-muted",
                ].join(" ")}
              >
                <span className={`shrink-0 font-mono text-[10px] w-4 text-center ${status.tone}`}>
                  {status.label}
                </span>
                <span className="font-mono text-[12px] truncate flex-1 min-w-0">
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
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <div className="text-center text-xs text-fg-subtle py-6 px-4">{children}</div>;
}

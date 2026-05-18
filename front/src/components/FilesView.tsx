import useSWR from "swr";
import { useEffect, useMemo, useState } from "react";
import { Button, Spinner } from "@/components/ui";
import { IconBranch, IconRefresh } from "@/components/icons";

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

type DiffMode = "worktree" | "staged" | "head";

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

export function FilesView({ cwd, fullWidth = false }: { cwd: string; fullWidth?: boolean }) {
  const key = cwd ? `/api/git/status?path=${encodeURIComponent(cwd)}` : null;
  const { data, error, isLoading, mutate } = useSWR<GitStatus>(key, fetcher, {
    refreshInterval: 8000,
  });

  const [selected, setSelected] = useState<{
    file: string;
    mode: DiffMode;
  } | null>(null);

  // Reset selection when cwd changes
  useEffect(() => {
    setSelected(null);
  }, [cwd]);

  // If selected file disappears from status, clear selection.
  useEffect(() => {
    if (!selected || !data?.files) return;
    const exists = data.files.some(
      (f) => f.path === selected.file || (f.origPath && f.origPath === selected.file),
    );
    if (!exists) setSelected(null);
  }, [data, selected]);

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
        <FileList
          files={files}
          selected={selected}
          onSelect={(file, mode) => setSelected({ file, mode })}
        />
      )}

      {selected && (
        <DiffPanel
          cwd={cwd}
          file={selected.file}
          mode={selected.mode}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

function FileList({
  files,
  selected,
  onSelect,
}: {
  files: GitFile[];
  selected: { file: string; mode: DiffMode } | null;
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
        <FileGroup
          title="staged"
          mode="staged"
          files={grouped.staged}
          selected={selected}
          onSelect={onSelect}
        />
      )}
      {grouped.unstaged.length > 0 && (
        <FileGroup
          title="changes"
          mode="worktree"
          files={grouped.unstaged}
          selected={selected}
          onSelect={onSelect}
        />
      )}
      {grouped.untracked.length > 0 && (
        <FileGroup
          title="untracked"
          mode="worktree"
          files={grouped.untracked}
          selected={selected}
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
  selected,
  onSelect,
}: {
  title: string;
  mode: DiffMode;
  files: GitFile[];
  selected: { file: string; mode: DiffMode } | null;
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
          const isSel = selected?.file === f.path && selected.mode === mode;
          const status = fileStatusLabel(f);
          return (
            <li key={`${mode}:${f.path}`}>
              <button
                type="button"
                onClick={() => onSelect(f.path, mode)}
                className={[
                  "w-full text-left px-2.5 py-1.5 flex items-center gap-2",
                  "border-b border-line last:border-b-0",
                  isSel
                    ? "bg-accent/10 text-fg"
                    : "bg-surface hover:bg-surface-muted text-fg-muted",
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

function DiffPanel({
  cwd,
  file,
  mode,
  onClose,
}: {
  cwd: string;
  file: string;
  mode: DiffMode;
  onClose: () => void;
}) {
  const url =
    `/api/git/diff?path=${encodeURIComponent(cwd)}` +
    `&file=${encodeURIComponent(file)}&mode=${mode}`;
  const { data, error, isLoading } = useSWR<{
    diff: string;
    truncated: boolean;
    isUntracked: boolean;
  }>(url, fetcher);

  return (
    <div className="rounded-md border border-line overflow-hidden">
      <div className="flex items-center gap-2 px-2.5 py-1.5 border-b border-line bg-surface-muted">
        <div className="font-mono text-[11px] text-fg truncate flex-1 min-w-0">{file}</div>
        <span className="text-[10px] text-fg-faint uppercase tracking-wider">{mode}</span>
        <button
          type="button"
          onClick={onClose}
          aria-label="閉じる"
          className="w-5 h-5 inline-flex items-center justify-center rounded text-fg-faint hover:text-fg hover:bg-surface-elev"
        >
          ×
        </button>
      </div>
      <div className="max-h-[60vh] overflow-auto scroll-thin">
        {isLoading ? (
          <div className="flex justify-center py-6">
            <Spinner size="sm" />
          </div>
        ) : error ? (
          <Empty>取得に失敗: {(error as Error).message}</Empty>
        ) : (
          <DiffBody text={data?.diff ?? ""} />
        )}
      </div>
    </div>
  );
}

function DiffBody({ text }: { text: string }) {
  if (!text) {
    return <Empty>(no diff)</Empty>;
  }
  // Tokenize line-by-line for hunk coloring.
  const lines = text.split("\n");
  return (
    <pre className="text-[11.5px] leading-snug font-mono whitespace-pre">
      {lines.map((l, i) => {
        let cls = "text-fg-muted";
        if (l.startsWith("+++") || l.startsWith("---")) cls = "text-fg-faint";
        else if (l.startsWith("@@")) cls = "text-violet-500";
        else if (l.startsWith("+")) cls = "text-emerald-500";
        else if (l.startsWith("-")) cls = "text-red-500";
        else if (l.startsWith("diff ") || l.startsWith("index ")) cls = "text-fg-faint";
        return (
          <div key={i} className={cls}>
            {l || " "}
          </div>
        );
      })}
    </pre>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <div className="text-center text-xs text-fg-subtle py-6 px-4">{children}</div>;
}

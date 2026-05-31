import useSWR from "swr";
import { useState } from "react";
import { IconChevronDown, IconChevronRight, IconFolder } from "@/components/icons";
import { Skeleton, SkeletonRows } from "@/components/ui";

type TreeEntry = { path: string; name: string; isDir: boolean };

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    const json = await r.json();
    if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
    return json;
  });

function treeKey(cwd: string, subdir?: string) {
  const base = `/api/git/tree?path=${encodeURIComponent(cwd)}`;
  return subdir ? `${base}&subdir=${encodeURIComponent(subdir)}` : base;
}

/**
 * Lazy file tree of the repo's HEAD. Each directory fetches its own
 * children only when expanded. Clicking a file opens it in an editor tab
 * via `onOpenFile` (given the absolute path).
 */
export function RepoTreeView({
  cwd,
  onOpenFile,
}: {
  cwd: string;
  onOpenFile?: (absPath: string) => void;
}) {
  const { data, error, isLoading } = useSWR<{ entries: TreeEntry[] }>(treeKey(cwd), fetcher);

  if (isLoading && !data) {
    return (
      <div className="p-3 space-y-2">
        <Skeleton style={{ height: 14, width: "30%" }} />
        <SkeletonRows n={6} rowHeight={24} className="mt-2" />
      </div>
    );
  }
  if (error) {
    return <Empty>取得に失敗: {(error as Error).message}</Empty>;
  }
  const entries = data?.entries ?? [];
  if (entries.length === 0) {
    return <Empty>ファイルがありません</Empty>;
  }

  return (
    <div className="flex-1 min-h-0 overflow-auto scroll-thin p-2 text-xs">
      {entries.map((e) => (
        <Node key={e.path} cwd={cwd} entry={e} depth={0} onOpenFile={onOpenFile} />
      ))}
    </div>
  );
}

function Node({
  cwd,
  entry,
  depth,
  onOpenFile,
}: {
  cwd: string;
  entry: TreeEntry;
  depth: number;
  onOpenFile?: (absPath: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const { data, isLoading } = useSWR<{ entries: TreeEntry[] }>(
    open && entry.isDir ? treeKey(cwd, entry.path) : null,
    fetcher,
  );

  const pad = { paddingLeft: depth * 14 + 4 };

  if (!entry.isDir) {
    return (
      <button
        type="button"
        onClick={() => onOpenFile?.(`${cwd.replace(/\/$/, "")}/${entry.path}`)}
        className="w-full text-left flex items-center gap-1.5 px-1 py-0.5 rounded hover:bg-surface-muted text-fg-muted"
        style={pad}
      >
        <span className="w-3.5 shrink-0" />
        <span className="font-mono truncate">{entry.name}</span>
      </button>
    );
  }

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full text-left flex items-center gap-1.5 px-1 py-0.5 rounded hover:bg-surface-muted"
        style={pad}
      >
        {open ? (
          <IconChevronDown className="w-3.5 h-3.5 shrink-0 text-fg-faint" />
        ) : (
          <IconChevronRight className="w-3.5 h-3.5 shrink-0 text-fg-faint" />
        )}
        <IconFolder className="w-3.5 h-3.5 shrink-0 text-fg-faint" />
        <span className="font-mono truncate">{entry.name}</span>
      </button>
      {open && (
        <div>
          {isLoading && (
            <div className="text-2xs text-fg-faint" style={{ paddingLeft: (depth + 1) * 14 + 22 }}>
              …
            </div>
          )}
          {(data?.entries ?? []).map((child) => (
            <Node
              key={child.path}
              cwd={cwd}
              entry={child}
              depth={depth + 1}
              onOpenFile={onOpenFile}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex-1 flex items-center justify-center text-center text-xs text-fg-subtle p-6">
      {children}
    </div>
  );
}

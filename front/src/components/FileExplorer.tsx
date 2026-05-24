import { useCallback, useState } from "react";
import useSWR from "swr";
import { IconChevronDown, IconChevronRight, IconFolder } from "@/components/icons";
import { Spinner } from "@/components/ui";

type FsEntry = {
  name: string;
  path: string;
  isDir: boolean;
};

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    const json = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
    return json;
  });

type Props = {
  cwd: string;
  onOpenFile: (path: string) => void;
};

export function FileExplorer({ cwd, onOpenFile }: Props) {
  if (!cwd) {
    return <div className="px-3 py-6 text-center text-xs text-fg-subtle">cwd 不明</div>;
  }
  return (
    <div className="text-sm py-2">
      <DirNode path={cwd} depth={0} initiallyOpen onOpenFile={onOpenFile} rootLabel="/" />
    </div>
  );
}

function DirNode({
  path,
  depth,
  initiallyOpen,
  onOpenFile,
  rootLabel,
}: {
  path: string;
  depth: number;
  initiallyOpen?: boolean;
  onOpenFile: (path: string) => void;
  rootLabel?: string;
}) {
  const [open, setOpen] = useState(!!initiallyOpen);
  const { data, isLoading, error } = useSWR<{ entries: FsEntry[] }>(
    open ? `/api/fs/tree?path=${encodeURIComponent(path)}` : null,
    fetcher,
  );

  const label = rootLabel ?? path.split("/").filter(Boolean).pop() ?? path;

  const toggle = useCallback(() => setOpen((v) => !v), []);

  return (
    <div>
      <button
        type="button"
        onClick={toggle}
        className="w-full flex items-center gap-1 px-2 py-1 hover:bg-surface-muted text-left"
        style={{ paddingLeft: 8 + depth * 12 }}
      >
        <span className="text-fg-faint shrink-0 w-3.5 inline-flex">
          {open ? (
            <IconChevronDown className="w-3 h-3" />
          ) : (
            <IconChevronRight className="w-3 h-3" />
          )}
        </span>
        <IconFolder className="w-3.5 h-3.5 text-fg-faint shrink-0" />
        <span className="truncate text-fg">{label}</span>
      </button>
      {open && (
        <div>
          {isLoading && (
            <div className="py-1 pl-6">
              <Spinner size="xs" />
            </div>
          )}
          {error && (
            <div className="text-xs text-danger pl-6 py-1">
              読み込みエラー: {(error as Error).message}
            </div>
          )}
          {data?.entries?.map((e) =>
            e.isDir ? (
              <DirNode key={e.path} path={e.path} depth={depth + 1} onOpenFile={onOpenFile} />
            ) : (
              <FileLeaf key={e.path} entry={e} depth={depth + 1} onOpen={onOpenFile} />
            ),
          )}
        </div>
      )}
    </div>
  );
}

function FileLeaf({
  entry,
  depth,
  onOpen,
}: {
  entry: FsEntry;
  depth: number;
  onOpen: (path: string) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onOpen(entry.path)}
      className="w-full flex items-center gap-1 px-2 py-1 hover:bg-surface-muted text-left"
      style={{ paddingLeft: 8 + depth * 12 }}
    >
      <span className="w-3.5 shrink-0" />
      <span className="w-3.5 h-3.5 inline-block shrink-0" />
      <span className="truncate text-fg-muted text-[12px]">{entry.name}</span>
    </button>
  );
}

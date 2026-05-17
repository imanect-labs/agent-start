"use client";

import { Badge, Button } from "@/components/ui";
import { formatRelative } from "@/lib/format";

export type SessionCardProps = {
  name: string;
  path: string;
  createdAt: number;
  attached: boolean;
  cli: string;
  worktreePath: string;
  origPath: string;
  onPreview: () => void;
  onStop: () => void;
};

const CLI_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  shell: "Terminal",
};

export function SessionCard({
  name,
  path,
  createdAt,
  attached,
  cli,
  worktreePath,
  origPath,
  onPreview,
  onStop,
}: SessionCardProps) {
  const hasWorktree = !!worktreePath;
  const displayName = origPath
    ? origPath.split("/").pop()
    : path.split("/").pop();

  return (
    <div className="bg-white border border-zinc-200 rounded-lg p-3.5">
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="text-sm font-medium text-zinc-900 truncate">
              {displayName}
            </span>
            <Badge tone="violet">{CLI_LABEL[cli] || cli || "claude"}</Badge>
            {hasWorktree && <Badge tone="amber">worktree</Badge>}
            {attached && <Badge tone="blue">接続中</Badge>}
          </div>
          <div className="text-xs text-zinc-500 font-mono truncate mt-1">
            {name}
          </div>
          <div className="text-[11px] text-zinc-400 mt-0.5">
            起動 {formatRelative(createdAt)}
          </div>
          {hasWorktree && origPath && (
            <div className="text-[11px] text-zinc-400 truncate mt-0.5">
              元 <span className="font-mono">{origPath}</span>
            </div>
          )}
        </div>
      </div>

      <div className="flex gap-2 mt-3">
        <Button variant="secondary" size="md" onClick={onPreview} className="flex-1">
          ターミナル
        </Button>
        <Button variant="dangerOutline" size="md" onClick={onStop} className="flex-1">
          停止
        </Button>
      </div>
    </div>
  );
}

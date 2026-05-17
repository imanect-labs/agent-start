"use client";

import { Button } from "@heroui/react";
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
    <div className="bg-white border border-zinc-200 rounded-xl p-4">
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-semibold text-zinc-900 truncate">
              {displayName}
            </span>
            <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-zinc-100 text-zinc-700 border border-zinc-200">
              {CLI_LABEL[cli] || cli || "claude"}
            </span>
            {hasWorktree && (
              <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-amber-50 text-amber-700 border border-amber-200">
                worktree
              </span>
            )}
            {attached && (
              <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-blue-50 text-blue-700 border border-blue-200">
                接続中
              </span>
            )}
          </div>
          <div className="text-xs text-zinc-500 font-mono truncate mt-1">
            {name}
          </div>
          <div className="text-xs text-zinc-400 mt-0.5">
            起動 {formatRelative(createdAt)}
          </div>
          {hasWorktree && origPath && (
            <div className="text-xs text-zinc-400 truncate mt-0.5">
              元 {origPath}
            </div>
          )}
        </div>
      </div>

      <div className="flex gap-2 mt-3">
        <Button
          size="md"
          variant="bordered"
          radius="md"
          disableRipple
          className="flex-1 min-h-11 border-zinc-300"
          onPress={onPreview}
        >
          プレビュー
        </Button>
        <Button
          size="md"
          color="danger"
          variant="bordered"
          radius="md"
          disableRipple
          className="flex-1 min-h-11"
          onPress={onStop}
        >
          停止
        </Button>
      </div>
    </div>
  );
}

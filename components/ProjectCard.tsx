"use client";

import { Button } from "@heroui/react";
import { formatRelative } from "@/lib/format";

export type ProjectCardProps = {
  name: string;
  path: string;
  root: string;
  mtimeMs: number;
  isGit: boolean;
  onSelect: () => void;
};

export function ProjectCard({
  name,
  path,
  mtimeMs,
  isGit,
  onSelect,
}: ProjectCardProps) {
  return (
    <div className="flex items-center gap-3 px-4 py-3 bg-white border border-zinc-200 rounded-xl">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="font-semibold text-zinc-900 truncate">{name}</span>
          {isGit && (
            <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 border border-emerald-200">
              git
            </span>
          )}
        </div>
        <div className="text-xs text-zinc-500 truncate mt-0.5">{path}</div>
        <div className="text-xs text-zinc-400 mt-0.5">
          {formatRelative(mtimeMs)}
        </div>
      </div>
      <Button
        color="primary"
        size="md"
        radius="md"
        disableRipple
        onPress={onSelect}
        className="shrink-0 min-h-11 px-4 font-semibold"
      >
        起動
      </Button>
    </div>
  );
}

"use client";

import { Badge, Button } from "@/components/ui";
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
    <div className="group flex items-center gap-3 px-3.5 py-3 bg-white border border-zinc-200 rounded-lg hover:border-zinc-300 transition-colors">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="text-sm font-medium text-zinc-900 truncate">
            {name}
          </span>
          {isGit && <Badge tone="emerald">git</Badge>}
        </div>
        <div className="text-xs text-zinc-500 font-mono truncate mt-0.5">
          {path}
        </div>
        <div className="text-[11px] text-zinc-400 mt-0.5">
          {formatRelative(mtimeMs)}
        </div>
      </div>
      <Button variant="primary" size="md" onClick={onSelect} className="shrink-0">
        起動
      </Button>
    </div>
  );
}

"use client";

import { FilesView } from "@/components/FilesView";
import { IconX } from "@/components/icons";

type Props = {
  cwd: string;
  onClose: () => void;
};

export function RightPane({ cwd, onClose }: Props) {
  return (
    <aside className="w-80 shrink-0 h-full flex flex-col border-l border-line bg-surface">
      <div className="px-3 py-2 flex items-center gap-2 border-b border-line">
        <div className="text-[10px] uppercase tracking-wider text-fg-subtle font-medium flex-1">
          変更
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label="閉じる"
          className="w-7 h-7 inline-flex items-center justify-center rounded-md text-fg-faint hover:text-fg hover:bg-surface-muted"
        >
          <IconX className="w-3.5 h-3.5" />
        </button>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        <FilesView cwd={cwd} />
      </div>
    </aside>
  );
}

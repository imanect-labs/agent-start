import { useEffect, useState } from "react";
import { FilesView } from "@/components/FilesView";
import { FileExplorer } from "@/components/FileExplorer";
import { IconX } from "@/components/icons";
import type { DiffMode } from "@/components/tab-types";

type Props = {
  cwd: string;
  onClose: () => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (file: string, mode: DiffMode) => void;
  /** "inline" fills its parent (used inside a Panel on desktop).
   *  "overlay" pins to the right edge as a drawer with a backdrop (mobile/tablet). */
  mode?: "inline" | "overlay";
};

type Sub = "changes" | "files";

export function RightPane({ cwd, onClose, onOpenFile, onOpenDiff, mode = "inline" }: Props) {
  const [sub, setSub] = useState<Sub>("changes");

  useEffect(() => {
    if (mode !== "overlay") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [mode, onClose]);

  const body = (
    <aside
      className={[
        "h-full flex flex-col border-l border-line bg-surface",
        mode === "overlay"
          ? "fixed inset-y-0 right-0 z-40 w-[min(420px,100vw)] shadow-xl overscroll-contain safe-bottom"
          : "w-full",
      ].join(" ")}
    >
      <div className="px-2 py-1.5 flex items-center gap-1 border-b border-line">
        <SubTab active={sub === "changes"} onClick={() => setSub("changes")}>
          変更
        </SubTab>
        <SubTab active={sub === "files"} onClick={() => setSub("files")}>
          ファイル
        </SubTab>
        <div className="flex-1" />
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
        {sub === "changes" ? (
          <FilesView cwd={cwd} onOpenDiff={onOpenDiff} />
        ) : (
          <FileExplorer cwd={cwd} onOpenFile={(p) => onOpenFile?.(p)} />
        )}
      </div>
    </aside>
  );

  if (mode === "overlay") {
    return (
      <>
        <div className="fixed inset-0 z-30 bg-black/40" onClick={onClose} aria-hidden />
        {body}
      </>
    );
  }
  return body;
}

function SubTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "px-2.5 py-1 rounded text-2xs uppercase tracking-wider font-medium transition-colors",
        active ? "bg-surface-muted text-fg" : "text-fg-subtle hover:text-fg",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

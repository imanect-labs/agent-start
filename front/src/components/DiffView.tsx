import { useEffect, useMemo, useState } from "react";
import { Diff, Hunk, parseDiff, type ViewType, type HunkData } from "react-diff-view";
import "react-diff-view/style/index.css";

type FileData = {
  oldPath?: string;
  newPath?: string;
  type: "add" | "delete" | "modify" | "rename" | "copy";
  hunks: HunkData[];
};

function useResponsiveView(preferred: ViewType): ViewType {
  const [view, setView] = useState<ViewType>(preferred);
  useEffect(() => {
    if (typeof window === "undefined") return;
    const mq = window.matchMedia("(max-width: 900px)");
    const update = () => setView(mq.matches ? "unified" : preferred);
    update();
    mq.addEventListener("change", update);
    return () => mq.removeEventListener("change", update);
  }, [preferred]);
  return view;
}

export function DiffView({ text }: { text: string }) {
  const [pref, setPref] = useState<ViewType>("split");
  const view = useResponsiveView(pref);

  const files: FileData[] = useMemo(() => {
    if (!text.trim()) return [];
    try {
      return parseDiff(text);
    } catch {
      return [];
    }
  }, [text]);

  if (!text.trim()) {
    return <div className="p-6 text-center text-xs text-fg-subtle">(no diff)</div>;
  }
  if (files.length === 0) {
    return <UnifiedRaw text={text} />;
  }

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <div className="inline-flex rounded-md border border-line overflow-hidden text-2xs">
          <ToggleBtn active={pref === "split"} onClick={() => setPref("split")}>
            Split
          </ToggleBtn>
          <ToggleBtn active={pref === "unified"} onClick={() => setPref("unified")}>
            Unified
          </ToggleBtn>
        </div>
      </div>
      {files.map((f, i) => (
        <FileBlock key={`${f.oldPath ?? "x"}-${f.newPath ?? "x"}-${i}`} file={f} view={view} />
      ))}
    </div>
  );
}

function FileBlock({ file, view }: { file: FileData; view: ViewType }) {
  const title = file.newPath ?? file.oldPath ?? "(unknown)";
  return (
    <div className="border border-line rounded-md overflow-hidden bg-surface">
      <div className="px-3 py-2 border-b border-line bg-surface-muted text-xs font-mono truncate flex items-center gap-2">
        <span className="text-fg">{title}</span>
        {file.oldPath && file.newPath && file.oldPath !== file.newPath && (
          <span className="text-fg-faint text-2xs">← {file.oldPath}</span>
        )}
        <span className="ml-auto text-2xs uppercase tracking-wider text-fg-faint">{file.type}</span>
      </div>
      <div className="overflow-x-auto diff-wrap">
        <Diff viewType={view} diffType={file.type} hunks={file.hunks}>
          {(hunks) => hunks.map((h, i) => <Hunk key={i} hunk={h} />)}
        </Diff>
      </div>
    </div>
  );
}

function ToggleBtn({
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
        "px-2.5 h-6 transition-colors",
        active ? "bg-surface-muted text-fg" : "bg-surface text-fg-subtle hover:bg-surface-muted",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

function UnifiedRaw({ text }: { text: string }) {
  return (
    <pre className="text-xs leading-[1.5] font-mono whitespace-pre overflow-x-auto p-3 bg-surface border border-line rounded-md">
      {text.split("\n").map((l, i) => {
        let cls = "text-fg-muted";
        if (l.startsWith("+++") || l.startsWith("---")) cls = "text-fg-faint";
        else if (l.startsWith("@@")) cls = "text-fg-subtle";
        else if (l.startsWith("+")) cls = "text-add";
        else if (l.startsWith("-")) cls = "text-del";
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

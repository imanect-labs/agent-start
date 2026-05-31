import { Terminal } from "@/components/Terminal";
import { ChatView, type ChatModelInfo } from "@/components/ChatView";
import { FilesView } from "@/components/FilesView";
import { EditorTab as EditorView } from "@/components/EditorTab";
import { DiffTabView } from "@/components/DiffTabView";
import { GuiView } from "@/components/GuiView";
import { CommitGraphView } from "@/components/CommitGraphView";
import { RepoTreeView } from "@/components/RepoTreeView";
import type { DiffMode, Tab } from "@/components/tab-types";
import { TerminalSkeleton } from "./skeletons";

export function TabContent({
  tab,
  sessionName,
  sessionStopped,
  restarting,
  onRestartSession,
  cwd,
  onUpdateTab,
  onOpenDiff,
  onOpenFile,
  chatModels,
  chatDefaultModel,
}: {
  tab: Tab;
  sessionName: string;
  sessionStopped: boolean;
  restarting: boolean;
  onRestartSession: () => void;
  cwd: string;
  onUpdateTab: (id: string, patch: Partial<Tab>) => void;
  onOpenDiff?: (file: string, mode: DiffMode) => void;
  onOpenFile?: (absPath: string) => void;
  chatModels: ChatModelInfo[];
  chatDefaultModel: string | null;
}) {
  if (tab.kind === "chat") {
    return (
      <ChatView
        key={sessionName}
        sessionName={sessionName}
        cwd={cwd}
        models={chatModels}
        defaultModel={chatDefaultModel}
      />
    );
  }
  if (tab.kind === "terminal") {
    // Optimistic terminal tab: the tmux window is still being created on the
    // host. Show a skeleton until the real windowId arrives and the tab
    // remounts onto a live PTY.
    if (tab.pending || tab.windowId < 0) {
      return (
        <div className="flex-1 min-h-0 p-3">
          <TerminalSkeleton />
        </div>
      );
    }
    return (
      <div className="flex-1 min-h-0 p-3">
        <Terminal
          // Including `sessionStopped` in the key forces a fresh xterm + WS when
          // the user clicks "再開" (stopped → live) so the snapshot pane is
          // replaced by the new PTY's output.
          key={`${sessionName}:${tab.windowId}:${sessionStopped ? "s" : "l"}`}
          sessionName={sessionName}
          windowId={tab.windowId}
          stopped={sessionStopped}
          restarting={restarting}
          onRestart={onRestartSession}
        />
      </div>
    );
  }
  if (tab.kind === "files") {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        <FilesView cwd={cwd} fullWidth onOpenDiff={onOpenDiff} />
      </div>
    );
  }
  if (tab.kind === "gui") {
    return <GuiView sessionName={sessionName} />;
  }
  if (tab.kind === "graph") {
    return <CommitGraphView cwd={tab.cwd} />;
  }
  if (tab.kind === "tree") {
    return <RepoTreeView cwd={tab.cwd} onOpenFile={onOpenFile} />;
  }
  if (tab.kind === "diff") {
    return (
      <DiffTabView
        key={`${tab.cwd}:${tab.file}:${tab.mode}`}
        cwd={tab.cwd}
        file={tab.file}
        mode={tab.mode}
      />
    );
  }
  return (
    <EditorView
      key={tab.path}
      path={tab.path}
      view={tab.view}
      onViewChange={(v) => onUpdateTab(tab.id, { view: v } as Partial<Tab>)}
      onDirtyChange={(d) => onUpdateTab(tab.id, { dirty: d } as Partial<Tab>)}
    />
  );
}

import { EmptyState } from "@/components/ui";
import { type ChatModelInfo } from "@/components/ChatView";
import type { Project, TmuxSession } from "@/components/Sidebar";
import type { DiffMode, SessionTabs, Tab } from "@/components/tab-types";
import { SessionHeader } from "@/components/main/SessionHeader";
import { TabBar } from "@/components/main/TabBar";
import { TabContent } from "@/components/main/TabContent";
import { PendingSessionView } from "@/components/main/PendingSessionView";
import { WelcomeBanner, type RecentProject } from "@/components/main/WelcomeBanner";

// Re-exported for existing importers (e.g. routes/index.tsx).
export type { RecentProject };

type Props = {
  session: TmuxSession | null;
  tabs: SessionTabs | null;
  onSelectTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onAddTerminal: () => void;
  onAddFiles: () => void;
  onAddGui: () => void;
  onAddGraph: () => void;
  onAddTree: () => void;
  onStopSession: (s: TmuxSession) => void;
  onRestartSession: (s: TmuxSession) => void;
  restarting?: boolean;
  rightPaneOpen: boolean;
  onToggleRightPane: () => void;
  /** Update an editor tab's view/dirty/etc. */
  onUpdateTab: (id: string, patch: Partial<Tab>) => void;
  /** Toggle sidebar visibility (mobile drawer). */
  onToggleSidebar?: () => void;
  /** Open a diff for a changed file as a tab. */
  onOpenDiff?: (file: string, mode: DiffMode) => void;
  /** Open a file (absolute path) in an editor tab — used by the tree view. */
  onOpenFile?: (absPath: string) => void;
  /** Chat model menu + default for chat-mode sessions (#34). */
  chatModels?: ChatModelInfo[];
  chatDefaultModel?: string | null;
  /** Recently-used projects shown on the welcome screen — sorted by the
   *  most recent session's createdAt. Empty list hides the section. */
  recentProjects?: RecentProject[];
  onLaunchProject?: (p: Project) => void;
  onOpenSession?: (name: string) => void;
};

export function MainPane({
  session,
  tabs,
  onSelectTab,
  onCloseTab,
  onAddTerminal,
  onAddFiles,
  onAddGui,
  onAddGraph,
  onAddTree,
  onStopSession,
  onRestartSession,
  restarting,
  rightPaneOpen,
  onToggleRightPane,
  onUpdateTab,
  onToggleSidebar,
  onOpenDiff,
  onOpenFile,
  chatModels,
  chatDefaultModel,
  recentProjects,
  onLaunchProject,
  onOpenSession,
}: Props) {
  // Optimistic placeholder while the host is still creating the session
  // (POST /api/sessions in flight). The real PTY/name doesn't exist yet, so
  // we can't mount a Terminal — show a skeleton shaped like the eventual
  // layout so the launch feels instant.
  if (session?.pending) {
    // The temp id ("pending:…") isn't user-facing — show the project's
    // basename while the real session name is being assigned.
    const label = session.path.split("/").filter(Boolean).pop() || "セッション";
    return <PendingSessionView name={label} onToggleSidebar={onToggleSidebar} />;
  }

  if (!session || !tabs) {
    return (
      <WelcomeBanner
        onToggleSidebar={onToggleSidebar}
        recentProjects={recentProjects ?? []}
        onLaunchProject={onLaunchProject}
        onOpenSession={onOpenSession}
      />
    );
  }

  const active = tabs.tabs.find((t) => t.id === tabs.activeTabId) ?? null;
  const cwd = session.worktreePath || session.path;

  return (
    <div className="h-full w-full min-w-0 flex flex-col bg-app">
      <SessionHeader
        session={session}
        restarting={!!restarting}
        rightPaneOpen={rightPaneOpen}
        onToggleRightPane={onToggleRightPane}
        onToggleSidebar={onToggleSidebar}
        onStopSession={() => onStopSession(session)}
        onRestartSession={() => onRestartSession(session)}
      />

      <TabBar
        tabs={tabs.tabs}
        activeId={tabs.activeTabId}
        onSelect={onSelectTab}
        onClose={(id) => {
          const t = tabs.tabs.find((x) => x.id === id);
          if (t && t.kind === "editor" && t.dirty) {
            if (!window.confirm("未保存の変更があります。破棄してタブを閉じますか?")) return;
          }
          onCloseTab(id);
        }}
        onAddTerminal={onAddTerminal}
        onAddFiles={onAddFiles}
        onAddGui={onAddGui}
        onAddGraph={onAddGraph}
        onAddTree={onAddTree}
        canAddFiles={!!cwd}
      />

      {/* Tab contents. All tabs stay mounted (hidden when inactive) so tab
          switching never tears down xterm / CodeMirror / diff state. Terminal
          WebSockets stay open in the background; if a project regularly opens
          many terminals this may need an LRU cap. */}
      <div className="flex-1 min-h-0 relative">
        {tabs.tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center">
            <EmptyState title="タブを選択してください" compact />
          </div>
        )}
        {tabs.tabs.map((t) => (
          <div
            key={t.id}
            className={["absolute inset-0 flex flex-col", t.id === active?.id ? "" : "hidden"].join(
              " ",
            )}
            // aria-hidden prevents inactive panels from receiving focus when
            // users tab through the live tab.
            aria-hidden={t.id !== active?.id}
          >
            <TabContent
              tab={t}
              sessionName={session.name}
              sessionStopped={!!session.stopped}
              restarting={!!restarting}
              onRestartSession={() => onRestartSession(session)}
              cwd={cwd}
              onUpdateTab={onUpdateTab}
              onOpenDiff={onOpenDiff}
              onOpenFile={onOpenFile}
              chatModels={chatModels ?? []}
              chatDefaultModel={chatDefaultModel ?? null}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

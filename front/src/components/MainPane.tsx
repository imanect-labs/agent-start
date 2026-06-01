import { useEffect, useRef, useState } from "react";
import { Badge, Button, Skeleton, Spinner } from "@/components/ui";
import {
  IconBranch,
  IconChevronRight,
  IconFolder,
  IconPlus,
  IconStop,
  IconTerminal,
  IconX,
} from "@/components/icons";
import { Terminal } from "@/components/Terminal";
import { ChatView, type ChatModelInfo } from "@/components/ChatView";
import { FilesView } from "@/components/FilesView";
import { EditorTab as EditorView } from "@/components/EditorTab";
import { DiffTabView } from "@/components/DiffTabView";
import { GuiView } from "@/components/GuiView";
import { CommitGraphView } from "@/components/CommitGraphView";
import { RepoTreeView } from "@/components/RepoTreeView";
import type { Project, TmuxSession } from "@/components/Sidebar";
import type { DiffMode, SessionTabs, Tab } from "@/components/tab-types";
import { formatRelative } from "@/lib/format";
import { useToast } from "@/components/Toast";

const CLI_LABEL: Record<string, string> = {
  claude: "Claude Code",
  "claude-chat": "Claude Code (Chat)",
  codex: "Codex CLI",
  shell: "Shell",
};

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

export type RecentProject = {
  project: Project;
  lastSessionName?: string;
  lastSessionAt: number;
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
  const cliLabel = CLI_LABEL[session.cli] || session.cli;
  const sessionTitle = session.title?.trim();
  const hasWorktree = !!session.worktreePath;
  const cwd = session.worktreePath || session.path;
  const toast = useToast();
  const [openingVscode, setOpeningVscode] = useState(false);

  const handleOpenVscode = async () => {
    if (openingVscode) return;
    setOpeningVscode(true);
    // window.open must be called synchronously from a user gesture to
    // avoid the popup blocker; reserve the tab now and navigate it once
    // the host confirms code-server is up.
    const tab = window.open("about:blank", "_blank");
    try {
      const res = await fetch(`/api/sessions/${encodeURIComponent(session.name)}/code-server`, {
        method: "POST",
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error || `HTTP ${res.status}`);
      }
      const data = (await res.json()) as { url: string };
      if (tab) {
        tab.location.href = data.url;
      } else {
        window.location.href = data.url;
      }
    } catch (e: unknown) {
      if (tab) tab.close();
      toast({
        title: "VSCode を開けませんでした",
        message: e instanceof Error ? e.message : String(e),
        color: "danger",
      });
    } finally {
      setOpeningVscode(false);
    }
  };

  return (
    <div className="h-full w-full min-w-0 flex flex-col bg-app">
      {/* Session header — tighter padding on mobile so the row holds its
          height even with a long session name. Badges below sm only show
          a colored status dot to keep everything on one line. */}
      <div className="flex items-center gap-2 sm:gap-3 px-3 sm:px-4 py-2 border-b border-line bg-surface min-h-[52px]">
        {onToggleSidebar && (
          <button
            type="button"
            onClick={onToggleSidebar}
            aria-label="サイドバーを開く"
            className="lg:hidden -ml-1 w-10 h-10 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted"
          >
            <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4">
              <path d="M3 5h14v2H3V5Zm0 4h14v2H3V9Zm0 4h14v2H3v-2Z" />
            </svg>
          </button>
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            {/* Mobile: tiny status dot (attached/stopped). Replaces the
                wrapping badge row that doubled the header height. */}
            <span
              aria-hidden
              className={[
                "sm:hidden inline-block w-1.5 h-1.5 rounded-full shrink-0",
                session.stopped ? "bg-warn" : session.attached ? "bg-success" : "bg-fg-faint",
              ].join(" ")}
            />
            <span
              className={["text-sm text-fg truncate", sessionTitle ? "" : "font-mono"].join(" ")}
              title={sessionTitle ? `${sessionTitle}\n${session.name}` : session.name}
            >
              {sessionTitle || session.name}
            </span>
            <div className="hidden sm:flex items-center gap-1.5 flex-wrap">
              <Badge tone="violet">{cliLabel}</Badge>
              {hasWorktree && <Badge tone="amber">worktree</Badge>}
              {session.stopped ? (
                <Badge tone="amber">停止中 (再起動後)</Badge>
              ) : (
                session.attached && <Badge tone="blue">接続中</Badge>
              )}
            </div>
          </div>
          <div className="text-[11px] text-fg-subtle truncate mt-0.5 font-mono">
            {session.origPath || session.path}
          </div>
        </div>
        <button
          type="button"
          onClick={onToggleRightPane}
          className={[
            "shrink-0 h-10 sm:h-8 inline-flex items-center justify-center gap-1.5 rounded-md border",
            "w-10 sm:w-auto sm:px-2.5 text-xs",
            rightPaneOpen
              ? "bg-surface-muted text-fg border-line-strong"
              : "bg-surface text-fg-muted border-line hover:bg-surface-muted",
          ].join(" ")}
          aria-label={rightPaneOpen ? "右ペインを隠す" : "右ペインを表示"}
          title={rightPaneOpen ? "右ペインを隠す" : "右ペインを表示"}
        >
          <IconChevronRight
            className={["w-3.5 h-3.5 transition-transform", rightPaneOpen ? "" : "rotate-180"].join(
              " ",
            )}
          />
          <span className="hidden sm:inline">変更</span>
        </button>
        {/* Inline buttons on sm+; collapsed into a kebab menu on phones
            so the header doesn't wrap and badges stay on one row. */}
        <div className="hidden sm:flex items-center gap-2 shrink-0">
          {session.stopped && (
            <Button
              variant="secondary"
              size="sm"
              disabled={!!restarting}
              onClick={() => onRestartSession(session)}
            >
              {restarting ? "再開中…" : "再開"}
            </Button>
          )}
          <Button
            variant="secondary"
            size="sm"
            disabled={openingVscode || session.stopped}
            onClick={handleOpenVscode}
            title={
              session.stopped
                ? "停止中のセッションでは利用できません"
                : "ブラウザで VSCode (code-server) を開く"
            }
          >
            {openingVscode ? "起動中…" : "VSCode で開く"}
          </Button>
          <Button
            variant="dangerOutline"
            size="sm"
            onClick={() => onStopSession(session)}
            leftIcon={<IconStop className="w-3.5 h-3.5" />}
          >
            停止
          </Button>
        </div>
        <div className="sm:hidden">
          <HeaderActionsMenu
            sessionStopped={!!session.stopped}
            restarting={!!restarting}
            openingVscode={openingVscode}
            onRestart={() => onRestartSession(session)}
            onOpenVscode={handleOpenVscode}
            onStop={() => onStopSession(session)}
          />
        </div>
      </div>

      {/* Tab bar */}
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

      {/* Tab contents. All tabs stay mounted (hidden when inactive) so
          tab switching never tears down xterm / CodeMirror / diff state.
          Terminal WebSockets stay open in the background; if a project
          regularly opens many terminals this may need an LRU cap. */}
      <div className="flex-1 min-h-0 relative">
        {tabs.tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-fg-subtle text-sm">
            タブを選択してください
          </div>
        )}
        {tabs.tabs.map((t) => (
          <div
            key={t.id}
            className={["absolute inset-0 flex flex-col", t.id === active?.id ? "" : "hidden"].join(
              " ",
            )}
            // aria-hidden prevents inactive panels from receiving focus
            // when users tab through the live tab.
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

function TabBar({
  tabs,
  activeId,
  onSelect,
  onClose,
  onAddTerminal,
  onAddFiles,
  onAddGui,
  onAddGraph,
  onAddTree,
  canAddFiles,
}: {
  tabs: Tab[];
  activeId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onAddTerminal: () => void;
  onAddFiles: () => void;
  onAddGui: () => void;
  onAddGraph: () => void;
  onAddTree: () => void;
  canAddFiles: boolean;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [menuOpen]);

  return (
    // The bar must always be a single horizontal strip — `min-w-0`
    // lets the flex parent in <MainPane> actually clamp our width
    // (without it, the children's intrinsic size lets us push past
    // the column edge and the page itself starts scrolling). Inside,
    // `flex-nowrap` keeps tabs in a row and `overflow-x-auto`
    // promotes them to a horizontal scrollbar once they spill over.
    <div className="flex items-stretch min-w-0 border-b border-line bg-surface-muted">
      <div className="flex items-stretch flex-nowrap overflow-x-auto scroll-thin min-w-0 flex-1">
        {tabs.map((t) => {
          const isActive = t.id === activeId;
          let label = t.label;
          let icon = <IconTerminal className="w-3.5 h-3.5 shrink-0 text-fg-faint" />;
          let dirty = false;
          if (t.kind === "terminal") {
            // Number from the underlying PTY window index (stable for
            // the lifetime of the window — never reused even after
            // siblings are closed). Window 0 displays as "Terminal 1".
            label = label ?? `Terminal ${t.windowId + 1}`;
          } else if (t.kind === "files") {
            label = label ?? "Files";
            icon = <IconFolder className="w-3.5 h-3.5 shrink-0 text-fg-faint" />;
          } else if (t.kind === "gui") {
            label = label ?? "GUI";
            icon = (
              <span className="w-3.5 h-3.5 inline-flex items-center justify-center text-[10px] text-fg-faint">
                ▢
              </span>
            );
          } else if (t.kind === "diff") {
            const base = t.file.split("/").pop() || t.file;
            label = label ?? `${base} (diff)`;
            icon = (
              <span className="w-3.5 h-3.5 inline-flex items-center justify-center text-[10px] text-fg-faint">
                Δ
              </span>
            );
          } else if (t.kind === "graph") {
            label = label ?? "Graph";
            icon = <IconBranch className="w-3.5 h-3.5 shrink-0 text-fg-faint" />;
          } else if (t.kind === "tree") {
            label = label ?? "Tree";
            icon = <IconFolder className="w-3.5 h-3.5 shrink-0 text-fg-faint" />;
          } else if (t.kind === "chat") {
            label = label ?? "Chat";
            icon = (
              <span className="w-3.5 h-3.5 inline-flex items-center justify-center text-[11px] text-fg-faint">
                ◇
              </span>
            );
          } else {
            const base = t.path.split("/").pop() || t.path;
            label = label ?? base;
            dirty = !!t.dirty;
            icon = (
              <span className="w-3.5 h-3.5 inline-flex items-center text-[10px] text-fg-faint">
                ≡
              </span>
            );
          }
          return (
            <div
              key={t.id}
              className={[
                // shrink-0 is what actually prevents the wrap-into-rows
                // behaviour the user hit: without it, tabs squish to
                // their min content, and once min content > available
                // width the row reflows.
                "shrink-0 group flex items-center gap-2 pl-3 pr-1 py-2.5 sm:py-2 border-r border-line max-w-[140px] sm:max-w-[200px] cursor-pointer",
                isActive ? "bg-app text-fg" : "text-fg-muted hover:bg-surface",
              ].join(" ")}
              onClick={() => onSelect(t.id)}
            >
              {icon}
              <span className="text-[12px] truncate">
                {label}
                {dirty && <span className="ml-1 text-warn">•</span>}
              </span>
              <button
                type="button"
                aria-label="タブを閉じる"
                title="タブを閉じる"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(t.id);
                }}
                className="ml-1 w-7 h-7 sm:w-5 sm:h-5 inline-flex items-center justify-center rounded text-fg-faint hover:text-fg hover:bg-surface-elev"
              >
                <IconX className="w-3.5 h-3.5 sm:w-3 sm:h-3" />
              </button>
            </div>
          );
        })}
      </div>
      {/* "+" sticks to the right edge of the bar regardless of how
          many tabs are open — it's outside the scrolling region. */}
      <div ref={menuRef} className="relative shrink-0 border-l border-line">
        <button
          type="button"
          onClick={() => setMenuOpen((s) => !s)}
          className="h-full px-4 sm:px-3 inline-flex items-center text-fg-faint hover:text-fg hover:bg-surface"
          aria-label="タブを追加"
          title="タブを追加"
        >
          <IconPlus className="w-3.5 h-3.5" />
        </button>
        {menuOpen && (
          <div className="absolute top-full right-0 mt-1 z-20 w-56 bg-surface-elev border border-line rounded-md shadow-lg py-1">
            <MenuItem
              icon={<IconTerminal className="w-3.5 h-3.5" />}
              onClick={() => {
                setMenuOpen(false);
                onAddTerminal();
              }}
            >
              新規ターミナル
            </MenuItem>
            <MenuItem
              icon={<IconFolder className="w-3.5 h-3.5" />}
              disabled={!canAddFiles}
              onClick={() => {
                setMenuOpen(false);
                onAddFiles();
              }}
            >
              ファイル変更
              {!canAddFiles && <span className="ml-1 text-fg-faint text-[10px]">(cwd 不明)</span>}
            </MenuItem>
            <MenuItem
              icon={
                <span className="w-3.5 h-3.5 inline-flex items-center justify-center text-[10px]">
                  ▢
                </span>
              }
              onClick={() => {
                setMenuOpen(false);
                onAddGui();
              }}
            >
              GUI (noVNC)
            </MenuItem>
            <MenuItem
              icon={<IconBranch className="w-3.5 h-3.5" />}
              disabled={!canAddFiles}
              onClick={() => {
                setMenuOpen(false);
                onAddGraph();
              }}
            >
              コミットグラフ
              {!canAddFiles && <span className="ml-1 text-fg-faint text-[10px]">(cwd 不明)</span>}
            </MenuItem>
            <MenuItem
              icon={<IconFolder className="w-3.5 h-3.5" />}
              disabled={!canAddFiles}
              onClick={() => {
                setMenuOpen(false);
                onAddTree();
              }}
            >
              ファイルツリー
              {!canAddFiles && <span className="ml-1 text-fg-faint text-[10px]">(cwd 不明)</span>}
            </MenuItem>
          </div>
        )}
      </div>
    </div>
  );
}

function MenuItem({
  icon,
  onClick,
  disabled,
  children,
}: {
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      className={[
        "w-full text-left px-3 py-1.5 inline-flex items-center gap-2 text-xs",
        disabled ? "text-fg-faint cursor-not-allowed" : "text-fg hover:bg-surface-muted",
      ].join(" ")}
    >
      <span className="text-fg-faint">{icon}</span>
      {children}
    </button>
  );
}

function TabContent({
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
          // Including `sessionStopped` in the key forces a fresh xterm
          // + WS when the user clicks "再開" (stopped → live) so the
          // snapshot pane is replaced by the new PTY's output.
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

function HeaderActionsMenu({
  sessionStopped,
  restarting,
  openingVscode,
  onRestart,
  onOpenVscode,
  onStop,
}: {
  sessionStopped: boolean;
  restarting: boolean;
  openingVscode: boolean;
  onRestart: () => void;
  onOpenVscode: () => void;
  onStop: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-label="セッション操作"
        title="セッション操作"
        className="w-10 h-10 inline-flex items-center justify-center rounded-md border border-line bg-surface text-fg-muted hover:bg-surface-muted"
      >
        <span className="text-base leading-none">⋯</span>
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 z-30 w-44 bg-surface-elev border border-line rounded-md shadow-lg py-1">
          {sessionStopped && (
            <MenuButton
              onClick={() => {
                setOpen(false);
                onRestart();
              }}
              disabled={restarting}
            >
              {restarting ? "再開中…" : "再開"}
            </MenuButton>
          )}
          <MenuButton
            onClick={() => {
              setOpen(false);
              onOpenVscode();
            }}
            disabled={openingVscode || sessionStopped}
          >
            {openingVscode ? "起動中…" : "VSCode で開く"}
          </MenuButton>
          <MenuButton
            onClick={() => {
              setOpen(false);
              onStop();
            }}
            tone="danger"
          >
            停止
          </MenuButton>
        </div>
      )}
    </div>
  );
}

function MenuButton({
  onClick,
  disabled,
  tone,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  tone?: "danger";
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      className={[
        "w-full text-left px-3 py-1.5 text-xs",
        disabled
          ? "text-fg-faint cursor-not-allowed"
          : tone === "danger"
            ? "text-danger hover:bg-danger/5"
            : "text-fg hover:bg-surface-muted",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

/** Terminal-shaped skeleton: a status line plus a large dark rectangle that
 *  mirrors the xterm pane, used while a PTY/window is being created. */
function TerminalSkeleton() {
  return (
    <div className="flex flex-col gap-2 h-full min-h-0">
      <div className="flex items-center gap-1.5 text-xs">
        <Spinner size="sm" />
        <span className="text-fg-subtle">起動中…</span>
      </div>
      <Skeleton className="flex-1 min-h-0 rounded-md" />
    </div>
  );
}

/** Full-pane placeholder shown while POST /api/sessions is in flight: a
 *  header with the project name and a spinner, a skeleton tab bar, and a
 *  terminal-shaped skeleton body. */
function PendingSessionView({
  name,
  onToggleSidebar,
}: {
  name: string;
  onToggleSidebar?: () => void;
}) {
  return (
    <div className="h-full w-full min-w-0 flex flex-col bg-app" aria-live="polite" aria-busy="true">
      <div className="flex items-center gap-2 sm:gap-3 px-3 sm:px-4 py-2 border-b border-line bg-surface min-h-[52px]">
        {onToggleSidebar && (
          <button
            type="button"
            onClick={onToggleSidebar}
            aria-label="サイドバーを開く"
            className="lg:hidden -ml-1 w-10 h-10 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted"
          >
            <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4">
              <path d="M3 5h14v2H3V5Zm0 4h14v2H3V9Zm0 4h14v2H3v-2Z" />
            </svg>
          </button>
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <Spinner size="sm" />
            <span className="font-mono text-sm text-fg-muted truncate">{name}</span>
            <Badge tone="amber">起動中…</Badge>
          </div>
          <div className="text-[11px] text-fg-subtle mt-0.5">セッションを準備しています</div>
        </div>
      </div>
      {/* Skeleton tab bar */}
      <div className="flex items-stretch min-w-0 border-b border-line bg-surface-muted">
        <div className="shrink-0 flex items-center gap-2 pl-3 pr-4 py-2.5 sm:py-2 border-r border-line">
          <Skeleton className="w-3.5 h-3.5 rounded" />
          <Skeleton className="w-16 h-3 rounded" />
        </div>
      </div>
      <div className="flex-1 min-h-0 p-3">
        <TerminalSkeleton />
      </div>
    </div>
  );
}

function WelcomeBanner({
  onToggleSidebar,
  recentProjects,
  onLaunchProject,
  onOpenSession,
}: {
  onToggleSidebar?: () => void;
  recentProjects: RecentProject[];
  onLaunchProject?: (p: Project) => void;
  onOpenSession?: (name: string) => void;
}) {
  return (
    <div className="h-full w-full flex flex-col bg-app">
      {/* Mobile/tablet still needs a way to open the sidebar before any
          session exists — otherwise the user is stranded on the welcome
          screen with no entry point. Desktop hides this bar via lg:hidden. */}
      {onToggleSidebar && (
        <div className="lg:hidden flex items-center gap-2 px-3 py-2 border-b border-line bg-surface min-h-[52px]">
          <button
            type="button"
            onClick={onToggleSidebar}
            aria-label="サイドバーを開く"
            className="-ml-1 w-10 h-10 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted"
          >
            <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4">
              <path d="M3 5h14v2H3V5Zm0 4h14v2H3V9Zm0 4h14v2H3v-2Z" />
            </svg>
          </button>
          <span className="text-sm font-semibold tracking-tight text-fg">agent-start</span>
        </div>
      )}
      <div className="flex-1 overflow-y-auto scroll-thin">
        <div className="max-w-2xl mx-auto px-6 py-10">
          <div className="text-center">
            <div className="mx-auto w-14 h-14 rounded-xl bg-surface-muted border border-line flex items-center justify-center text-fg-subtle">
              <IconTerminal className="w-6 h-6" />
            </div>
            <h2 className="mt-4 text-base font-semibold text-fg">セッションが選択されていません</h2>
            <p className="mt-1 text-sm text-fg-subtle">
              左のサイドバーからプロジェクトを選び、 <span className="font-mono">＋</span>{" "}
              で新しいセッションを起動するか、稼働中のセッションをクリックしてターミナルを開きます。
            </p>
            <p className="mt-3 text-[11px] text-fg-faint inline-flex items-center gap-1">
              <IconBranch className="inline w-3 h-3" /> = worktree 付き
              <span className="mx-1">·</span>
              選択中のセッションには複数のタブとファイル変更ペインを開けます
            </p>
          </div>

          {recentProjects.length > 0 && (
            <section className="mt-10">
              <h3 className="text-xs font-semibold tracking-wide uppercase text-fg-subtle px-1">
                最近のプロジェクト
              </h3>
              <ul className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
                {recentProjects.map((r) => (
                  <li key={r.project.path}>
                    <button
                      type="button"
                      onClick={() => {
                        if (r.lastSessionName && onOpenSession) {
                          onOpenSession(r.lastSessionName);
                        } else if (onLaunchProject) {
                          onLaunchProject(r.project);
                        }
                      }}
                      className="w-full text-left px-3 py-2.5 rounded-md border border-line bg-surface hover:bg-surface-muted transition-colors flex items-start gap-2.5 min-w-0"
                    >
                      <IconFolder className="w-4 h-4 mt-0.5 shrink-0 text-fg-subtle" />
                      <div className="min-w-0 flex-1">
                        <div className="text-sm font-medium text-fg truncate">{r.project.name}</div>
                        <div className="text-[11px] text-fg-subtle truncate">{r.project.path}</div>
                        <div className="text-[11px] text-fg-faint mt-0.5">
                          {r.lastSessionName
                            ? `最終: ${formatRelative(r.lastSessionAt)}`
                            : "セッションなし"}
                        </div>
                      </div>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

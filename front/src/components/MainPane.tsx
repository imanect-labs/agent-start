import { useEffect, useRef, useState } from "react";
import { Badge, Button } from "@/components/ui";
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
import { FilesView } from "@/components/FilesView";
import type { TmuxSession } from "@/components/Sidebar";
import type { SessionTabs, Tab } from "@/components/tab-types";

const CLI_LABEL: Record<string, string> = {
  claude: "Claude Code",
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
  onStopSession: (s: TmuxSession) => void;
  rightPaneOpen: boolean;
  onToggleRightPane: () => void;
};

export function MainPane({
  session,
  tabs,
  onSelectTab,
  onCloseTab,
  onAddTerminal,
  onAddFiles,
  onStopSession,
  rightPaneOpen,
  onToggleRightPane,
}: Props) {
  if (!session || !tabs) {
    return <WelcomeBanner />;
  }

  const active = tabs.tabs.find((t) => t.id === tabs.activeTabId) ?? null;
  const cliLabel = CLI_LABEL[session.cli] || session.cli;
  const hasWorktree = !!session.worktreePath;
  const cwd = session.worktreePath || session.path;

  return (
    <div className="flex-1 min-w-0 flex flex-col bg-app">
      {/* Session header */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-line bg-surface">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="font-mono text-sm text-fg truncate">
              {session.name}
            </span>
            <Badge tone="violet">{cliLabel}</Badge>
            {hasWorktree && <Badge tone="amber">worktree</Badge>}
            {session.attached && <Badge tone="blue">接続中</Badge>}
          </div>
          <div className="text-[11px] text-fg-subtle truncate mt-0.5 font-mono">
            {session.origPath || session.path}
          </div>
        </div>
        <button
          type="button"
          onClick={onToggleRightPane}
          className={[
            "shrink-0 h-8 px-2.5 inline-flex items-center gap-1.5 rounded-md border text-xs",
            rightPaneOpen
              ? "bg-surface-muted text-fg border-line-strong"
              : "bg-surface text-fg-muted border-line hover:bg-surface-muted",
          ].join(" ")}
          title={rightPaneOpen ? "右ペインを隠す" : "右ペインを表示"}
        >
          <IconChevronRight
            className={[
              "w-3.5 h-3.5 transition-transform",
              rightPaneOpen ? "" : "rotate-180",
            ].join(" ")}
          />
          変更
        </button>
        <Button
          variant="dangerOutline"
          size="sm"
          onClick={() => onStopSession(session)}
          leftIcon={<IconStop className="w-3.5 h-3.5" />}
        >
          停止
        </Button>
      </div>

      {/* Tab bar */}
      <TabBar
        tabs={tabs.tabs}
        activeId={tabs.activeTabId}
        onSelect={onSelectTab}
        onClose={onCloseTab}
        onAddTerminal={onAddTerminal}
        onAddFiles={onAddFiles}
        canAddFiles={!!cwd}
      />

      {/* Active tab content */}
      <div className="flex-1 min-h-0 flex flex-col">
        {active ? (
          <TabContent tab={active} sessionName={session.name} cwd={cwd} />
        ) : (
          <div className="flex-1 flex items-center justify-center text-fg-subtle text-sm">
            タブを選択してください
          </div>
        )}
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
  canAddFiles,
}: {
  tabs: Tab[];
  activeId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onAddTerminal: () => void;
  onAddFiles: () => void;
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
          let icon = (
            <IconTerminal className="w-3.5 h-3.5 shrink-0 text-fg-faint" />
          );
          if (t.kind === "terminal") {
            // Number from the underlying PTY window index (stable for
            // the lifetime of the window — never reused even after
            // siblings are closed). Window 0 displays as "Terminal 1".
            label = label ?? `Terminal ${t.windowId + 1}`;
          } else {
            label = label ?? "Files";
            icon = (
              <IconFolder className="w-3.5 h-3.5 shrink-0 text-fg-faint" />
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
                "shrink-0 group flex items-center gap-2 pl-3 pr-1 py-2 border-r border-line max-w-[200px] cursor-pointer",
                isActive ? "bg-app text-fg" : "text-fg-muted hover:bg-surface",
              ].join(" ")}
              onClick={() => onSelect(t.id)}
            >
              {icon}
              <span className="text-[12px] truncate">{label}</span>
              <button
                type="button"
                aria-label="タブを閉じる"
                title="タブを閉じる"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(t.id);
                }}
                className="ml-1 w-5 h-5 inline-flex items-center justify-center rounded text-fg-faint hover:text-fg hover:bg-surface-elev"
              >
                <IconX className="w-3 h-3" />
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
          className="h-full px-3 inline-flex items-center text-fg-faint hover:text-fg hover:bg-surface"
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
              {!canAddFiles && (
                <span className="ml-1 text-fg-faint text-[10px]">
                  (cwd 不明)
                </span>
              )}
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
        disabled
          ? "text-fg-faint cursor-not-allowed"
          : "text-fg hover:bg-surface-muted",
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
  cwd,
}: {
  tab: Tab;
  sessionName: string;
  cwd: string;
}) {
  if (tab.kind === "terminal") {
    return (
      <div className="flex-1 min-h-0 p-3">
        <Terminal
          key={`${sessionName}:${tab.windowId}`}
          sessionName={sessionName}
          windowId={tab.windowId}
        />
      </div>
    );
  }
  return (
    <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
      <FilesView cwd={cwd} fullWidth />
    </div>
  );
}

function WelcomeBanner() {
  return (
    <div className="flex-1 flex items-center justify-center bg-app">
      <div className="text-center max-w-md px-6">
        <div className="mx-auto w-14 h-14 rounded-xl bg-surface-muted border border-line flex items-center justify-center text-fg-subtle">
          <IconTerminal className="w-6 h-6" />
        </div>
        <h2 className="mt-4 text-base font-semibold text-fg">
          セッションが選択されていません
        </h2>
        <p className="mt-1 text-sm text-fg-subtle">
          左のサイドバーからプロジェクトを選び、{" "}
          <span className="font-mono">＋</span>{" "}
          で新しいセッションを起動するか、稼働中のセッションをクリックしてターミナルを開きます。
        </p>
        <p className="mt-3 text-[11px] text-fg-faint inline-flex items-center gap-1">
          <IconBranch className="inline w-3 h-3" /> = worktree 付き
          <span className="mx-1">·</span>
          選択中のセッションには複数のタブとファイル変更ペインを開けます
        </p>
      </div>
    </div>
  );
}

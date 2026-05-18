"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button } from "@/components/ui";
import {
  IconBranch,
  IconChevronDown,
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
  // Decorate tabs with a stable, deterministic display label so the
  // active pill and the switcher menu render the same text. (Computing
  // ordinals lazily inside the JSX previously produced "Terminal 1"
  // in one place and "Terminal 2" in another when the active tab
  // wasn't first in the list.)
  const decorated = useMemo(() => decorateTabs(tabs), [tabs]);
  const activeDecorated =
    decorated.find((t) => t.id === activeId) ?? decorated[0];

  const [switchOpen, setSwitchOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const switchRef = useRef<HTMLDivElement>(null);
  const addRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!switchOpen && !addOpen) return;
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (switchOpen && !switchRef.current?.contains(t)) setSwitchOpen(false);
      if (addOpen && !addRef.current?.contains(t)) setAddOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [switchOpen, addOpen]);

  if (!activeDecorated) return null;

  return (
    <div className="flex items-center gap-1 px-2 py-1.5 border-b border-line bg-surface-muted">
      {/* Active tab pill + dropdown that lists every open tab. */}
      <div ref={switchRef} className="relative min-w-0">
        <button
          type="button"
          onClick={() => {
            setAddOpen(false);
            setSwitchOpen((s) => !s);
          }}
          className="group flex items-center gap-2 max-w-[260px] h-8 pl-2.5 pr-1.5 rounded-md border border-line bg-app text-fg hover:bg-surface text-xs"
          title={`タブ切替 (${decorated.length})`}
          aria-haspopup="menu"
          aria-expanded={switchOpen}
        >
          {activeDecorated.icon}
          <span className="truncate">{activeDecorated.label}</span>
          <span className="text-fg-faint text-[10px] ml-1">
            {decorated.length > 1 ? `${decorated.length}` : ""}
          </span>
          <IconChevronDown className="w-3 h-3 text-fg-faint shrink-0" />
        </button>
        {switchOpen && (
          <div
            role="menu"
            className="absolute top-full left-0 mt-1 z-20 w-64 max-h-72 overflow-y-auto scroll-thin bg-surface-elev border border-line rounded-md shadow-lg py-1"
          >
            {decorated.map((t) => {
              const isActive = t.id === activeId;
              return (
                <div
                  key={t.id}
                  className={[
                    "group w-full px-2 py-1.5 flex items-center gap-2 text-xs cursor-pointer",
                    isActive ? "bg-surface-muted" : "hover:bg-surface-muted",
                  ].join(" ")}
                  onClick={() => {
                    setSwitchOpen(false);
                    onSelect(t.id);
                  }}
                >
                  <span className="text-fg-faint shrink-0">{t.icon}</span>
                  <span className={`truncate flex-1 ${isActive ? "text-fg" : "text-fg-muted"}`}>
                    {t.label}
                  </span>
                  <button
                    type="button"
                    aria-label="タブを閉じる"
                    title="タブを閉じる"
                    onClick={(e) => {
                      e.stopPropagation();
                      onClose(t.id);
                    }}
                    className="w-5 h-5 inline-flex items-center justify-center rounded text-fg-faint opacity-0 group-hover:opacity-100 hover:text-fg hover:bg-surface-elev"
                  >
                    <IconX className="w-3 h-3" />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Close the active tab without opening the switcher. */}
      <button
        type="button"
        aria-label="アクティブなタブを閉じる"
        title="アクティブなタブを閉じる"
        onClick={() => onClose(activeDecorated.id)}
        className="w-7 h-7 inline-flex items-center justify-center rounded-md text-fg-faint hover:text-fg hover:bg-surface"
      >
        <IconX className="w-3.5 h-3.5" />
      </button>

      <div className="flex-1" />

      <div ref={addRef} className="relative">
        <button
          type="button"
          onClick={() => {
            setSwitchOpen(false);
            setAddOpen((s) => !s);
          }}
          className="h-8 px-2.5 inline-flex items-center gap-1 rounded-md border border-line bg-app text-fg-muted hover:bg-surface text-xs"
          aria-label="タブを追加"
          title="タブを追加"
        >
          <IconPlus className="w-3.5 h-3.5" />
          <IconChevronDown className="w-3 h-3 text-fg-faint" />
        </button>
        {addOpen && (
          <div
            role="menu"
            className="absolute top-full right-0 mt-1 z-20 w-56 bg-surface-elev border border-line rounded-md shadow-lg py-1"
          >
            <MenuItem
              icon={<IconTerminal className="w-3.5 h-3.5" />}
              onClick={() => {
                setAddOpen(false);
                onAddTerminal();
              }}
            >
              新規ターミナル
            </MenuItem>
            <MenuItem
              icon={<IconFolder className="w-3.5 h-3.5" />}
              disabled={!canAddFiles}
              onClick={() => {
                setAddOpen(false);
                onAddFiles();
              }}
            >
              ファイル変更
              {!canAddFiles && (
                <span className="ml-1 text-fg-faint text-[10px]">(cwd 不明)</span>
              )}
            </MenuItem>
          </div>
        )}
      </div>
    </div>
  );
}

type DecoratedTab = Tab & {
  label: string;
  icon: React.ReactNode;
};

function decorateTabs(tabs: Tab[]): DecoratedTab[] {
  const terminalCount = tabs.filter((t) => t.kind === "terminal").length;
  let termOrdinal = 0;
  return tabs.map((t) => {
    if (t.kind === "terminal") {
      termOrdinal += 1;
      const label =
        t.label ?? (terminalCount > 1 ? `Terminal ${termOrdinal}` : "Terminal");
      return {
        ...t,
        label,
        icon: <IconTerminal className="w-3.5 h-3.5 shrink-0 text-fg-faint" />,
      };
    }
    return {
      ...t,
      label: t.label ?? "Files",
      icon: <IconFolder className="w-3.5 h-3.5 shrink-0 text-fg-faint" />,
    };
  });
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

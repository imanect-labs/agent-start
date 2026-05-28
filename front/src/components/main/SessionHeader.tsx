import { useState } from "react";
import { Badge, Button, Menu, MenuButton, MenuItem, MenuList } from "@/components/ui";
import { IconChevronRight, IconStop } from "@/components/icons";
import type { TmuxSession } from "@/components/Sidebar";
import { useToast } from "@/components/Toast";
import { SidebarToggle } from "./SidebarToggle";

const CLI_LABEL: Record<string, string> = {
  claude: "Claude Code",
  "claude-chat": "Claude Code (Chat)",
  codex: "Codex CLI",
  shell: "Shell",
};

export function SessionHeader({
  session,
  restarting,
  rightPaneOpen,
  onToggleRightPane,
  onToggleSidebar,
  onStopSession,
  onRestartSession,
}: {
  session: TmuxSession;
  restarting: boolean;
  rightPaneOpen: boolean;
  onToggleRightPane: () => void;
  onToggleSidebar?: () => void;
  onStopSession: () => void;
  onRestartSession: () => void;
}) {
  const toast = useToast();
  const [openingVscode, setOpeningVscode] = useState(false);
  const cliLabel = CLI_LABEL[session.cli] || session.cli;
  const hasWorktree = !!session.worktreePath;

  const handleOpenVscode = async () => {
    if (openingVscode) return;
    setOpeningVscode(true);
    // window.open must be called synchronously from a user gesture to avoid the
    // popup blocker; reserve the tab now and navigate it once the host confirms
    // code-server is up.
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
      if (tab) tab.location.href = data.url;
      else window.location.href = data.url;
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
    <div className="flex items-center gap-2 sm:gap-3 px-3 sm:px-4 py-2 border-b border-line bg-surface/80 backdrop-blur-md min-h-[52px]">
      {onToggleSidebar && <SidebarToggle onToggle={onToggleSidebar} />}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          {/* Mobile: tiny status dot only, to keep the row on one line. */}
          <span
            aria-hidden
            className={[
              "sm:hidden inline-block w-1.5 h-1.5 rounded-full shrink-0",
              session.stopped ? "bg-warn" : session.attached ? "bg-success" : "bg-fg-faint",
            ].join(" ")}
          />
          <span className="font-mono text-sm text-fg truncate">{session.name}</span>
          <div className="hidden sm:flex items-center gap-1.5 flex-wrap">
            <Badge tone="neutral">{cliLabel}</Badge>
            {hasWorktree && <Badge tone="neutral">worktree</Badge>}
            {session.stopped ? (
              <Badge tone="amber" dot>
                停止中 (再起動後)
              </Badge>
            ) : (
              session.attached && (
                <Badge tone="emerald" dot>
                  接続中
                </Badge>
              )
            )}
          </div>
        </div>
        <div className="text-2xs text-fg-subtle truncate mt-0.5 font-mono">
          {session.origPath || session.path}
        </div>
      </div>
      <button
        type="button"
        onClick={onToggleRightPane}
        className={[
          "shrink-0 h-10 sm:h-8 inline-flex items-center justify-center gap-1.5 rounded border transition-colors",
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
      {/* Inline buttons on sm+; collapsed into a kebab menu on phones. */}
      <div className="hidden sm:flex items-center gap-2 shrink-0">
        {session.stopped && (
          <Button variant="secondary" size="sm" disabled={restarting} onClick={onRestartSession}>
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
          onClick={onStopSession}
          leftIcon={<IconStop className="w-3.5 h-3.5" />}
        >
          停止
        </Button>
      </div>
      <div className="sm:hidden">
        <Menu align="end">
          <MenuButton
            aria-label="セッション操作"
            className="inline-flex items-center justify-center w-10 h-10 rounded border border-line bg-surface text-fg-muted hover:bg-surface-muted transition-colors"
          >
            <span className="text-base leading-none">⋯</span>
          </MenuButton>
          <MenuList>
            {session.stopped && (
              <MenuItem onSelect={onRestartSession} disabled={restarting}>
                {restarting ? "再開中…" : "再開"}
              </MenuItem>
            )}
            <MenuItem onSelect={handleOpenVscode} disabled={openingVscode || session.stopped}>
              {openingVscode ? "起動中…" : "VSCode で開く"}
            </MenuItem>
            <MenuItem onSelect={onStopSession} tone="danger">
              停止
            </MenuItem>
          </MenuList>
        </Menu>
      </div>
    </div>
  );
}

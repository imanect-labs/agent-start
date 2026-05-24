import { useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge, Input, Spinner } from "@/components/ui";
import {
  IconBranch,
  IconChevronDown,
  IconChevronRight,
  IconFolder,
  IconGear,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconTerminal,
  IconX,
} from "@/components/icons";
import { formatRelative } from "@/lib/format";

export type Project = {
  name: string;
  path: string;
  root: string;
  mtimeMs: number;
  isGit: boolean;
};

export type PendingProject = {
  name: string;
  path: string;
  kind: string;
  error?: string;
};

export type TmuxSession = {
  name: string;
  path: string;
  createdAt: number;
  attached: boolean;
  cli: string;
  worktreePath: string;
  origPath: string;
};

const CLI_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  shell: "Shell",
};

type Group = {
  project: Project | null; // null = orphan group (sessions whose project root is gone)
  sessions: TmuxSession[];
  /** stable key for the group */
  key: string;
};

function sessionProjectPath(s: TmuxSession): string {
  return s.origPath || s.path;
}

export function Sidebar({
  projects,
  pending = [],
  sessions,
  loadingProjects,
  loadingSessions,
  activeSession,
  onLaunchProject,
  onOpenSession,
  onStopSession,
  onRefresh,
  onAddProject,
  onDeleteProject,
  open = true,
  onClose,
}: {
  projects: Project[];
  pending?: PendingProject[];
  sessions: TmuxSession[];
  loadingProjects: boolean;
  loadingSessions: boolean;
  activeSession: string | null;
  onLaunchProject: (p: Project) => void;
  onOpenSession: (name: string) => void;
  onStopSession: (s: TmuxSession) => void;
  onRefresh: () => void;
  onAddProject?: () => void;
  onDeleteProject?: (name: string) => void;
  /** Mobile drawer visibility. Always true on md+. */
  open?: boolean;
  onClose?: () => void;
}) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  // Build groups: one per project, sessions nested inside. Sessions whose
  // project root is unknown go in a synthetic "orphan" group.
  const groups: Group[] = useMemo(() => {
    const byPath = new Map<string, Group>();
    for (const p of projects) {
      byPath.set(p.path, { project: p, sessions: [], key: p.path });
    }
    const orphan: Group = { project: null, sessions: [], key: "__orphan__" };
    for (const s of sessions) {
      const pp = sessionProjectPath(s);
      const g = byPath.get(pp);
      if (g) g.sessions.push(s);
      else orphan.sessions.push(s);
    }
    // Sort sessions within each group by createdAt desc
    for (const g of byPath.values()) g.sessions.sort((a, b) => b.createdAt - a.createdAt);
    orphan.sessions.sort((a, b) => b.createdAt - a.createdAt);

    // Sort projects: those with running sessions first, then by mtimeMs desc
    const projectGroups = Array.from(byPath.values()).sort((a, b) => {
      const sa = a.sessions.length > 0 ? 1 : 0;
      const sb = b.sessions.length > 0 ? 1 : 0;
      if (sa !== sb) return sb - sa;
      return (b.project?.mtimeMs ?? 0) - (a.project?.mtimeMs ?? 0);
    });

    const out = projectGroups;
    if (orphan.sessions.length > 0) out.push(orphan);
    return out;
  }, [projects, sessions]);

  // Apply search filter on the project/session level
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return groups;
    return groups
      .map((g) => {
        const projHit =
          g.project &&
          (g.project.name.toLowerCase().includes(q) || g.project.path.toLowerCase().includes(q));
        const matchSessions = g.sessions.filter((s) => s.name.toLowerCase().includes(q));
        if (projHit) return g; // show whole group with all sessions
        if (matchSessions.length > 0) return { ...g, sessions: matchSessions };
        return null;
      })
      .filter((g): g is Group => g !== null);
  }, [groups, query]);

  // Initial expansion: groups with sessions, or the active session's group.
  useEffect(() => {
    setCollapsed((prev) => {
      const next = { ...prev };
      for (const g of groups) {
        if (next[g.key] !== undefined) continue;
        next[g.key] = g.sessions.length === 0; // start collapsed if no sessions
      }
      return next;
    });
  }, [groups]);

  // If a session becomes active, ensure its group is expanded.
  useEffect(() => {
    if (!activeSession) return;
    const s = sessions.find((x) => x.name === activeSession);
    if (!s) return;
    const groupKey = sessionProjectPath(s);
    setCollapsed((prev) => (prev[groupKey] ? { ...prev, [groupKey]: false } : prev));
  }, [activeSession, sessions]);

  const totalSessions = sessions.length;

  return (
    <>
      {open && onClose && (
        <div className="md:hidden fixed inset-0 z-30 bg-black/40" onClick={onClose} aria-hidden />
      )}
      <aside
        className={[
          "w-72 shrink-0 h-full flex flex-col border-r border-line bg-surface",
          "md:static md:translate-x-0",
          "fixed inset-y-0 left-0 z-40 transition-transform",
          open ? "translate-x-0" : "-translate-x-full",
        ].join(" ")}
      >
        <div className="px-3 py-3 flex items-center gap-2 border-b border-line">
          <div className="flex items-baseline gap-2 flex-1 min-w-0">
            <span className="text-sm font-semibold tracking-tight text-fg">agent-start</span>
            <span className="text-[10px] uppercase tracking-wider text-fg-faint">launcher</span>
          </div>
          <button
            type="button"
            onClick={onRefresh}
            aria-label="再読み込み"
            className="w-8 h-8 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted transition-colors"
          >
            <IconRefresh className="w-4 h-4" />
          </button>
          <Link
            to="/settings"
            aria-label="設定"
            className="w-8 h-8 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted transition-colors"
          >
            <IconGear className="w-4 h-4" />
          </Link>
        </div>

        <div className="px-3 py-3 border-b border-line">
          <Input
            type="search"
            placeholder="検索"
            value={query}
            onValueChange={setQuery}
            clearable
            leftSlot={<IconSearch className="w-4 h-4" />}
          />
          <div className="mt-2 flex items-center gap-2 text-[11px] text-fg-subtle">
            <span className="inline-flex items-center gap-1">
              <span className="inline-block w-1.5 h-1.5 rounded-full bg-success" />
              {totalSessions} セッション
            </span>
            <span className="text-fg-faint">·</span>
            <span>{projects.length} プロジェクト</span>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto scroll-thin">
          {pending.length > 0 && (
            <ul className="py-1.5 border-b border-line">
              {pending.map((p) => (
                <PendingRow
                  key={p.name + p.kind}
                  pending={p}
                  onCancel={() => onDeleteProject?.(p.name)}
                />
              ))}
            </ul>
          )}
          {(loadingProjects || loadingSessions) && groups.length === 0 ? (
            <div className="flex justify-center py-8">
              <Spinner size="sm" />
            </div>
          ) : filtered.length === 0 && pending.length === 0 ? (
            <div className="px-4 py-8 text-center text-xs text-fg-subtle">
              {query ? "一致する項目がありません" : "プロジェクトがありません"}
            </div>
          ) : (
            <ul className="py-1.5">
              {filtered.map((g) => (
                <GroupRow
                  key={g.key}
                  group={g}
                  expanded={!collapsed[g.key]}
                  onToggle={() => setCollapsed((prev) => ({ ...prev, [g.key]: !prev[g.key] }))}
                  activeSession={activeSession}
                  onLaunch={g.project ? () => onLaunchProject(g.project!) : null}
                  onOpenSession={onOpenSession}
                  onStopSession={onStopSession}
                  onDeleteProject={onDeleteProject}
                />
              ))}
            </ul>
          )}
        </div>

        {onAddProject && (
          <div className="border-t border-line p-2">
            <button
              type="button"
              onClick={onAddProject}
              className="w-full inline-flex items-center justify-center gap-1.5 h-9 rounded-md border border-line bg-surface hover:bg-surface-muted text-sm text-fg"
            >
              <IconPlus className="w-3.5 h-3.5" /> プロジェクトを追加
            </button>
          </div>
        )}
      </aside>
    </>
  );
}

function PendingRow({ pending, onCancel }: { pending: PendingProject; onCancel: () => void }) {
  const isError = !!pending.error;
  return (
    <li className="px-1.5">
      <div className="group flex items-center gap-2 px-1.5 py-1.5 rounded-md">
        <span className="w-5 h-5 inline-flex items-center justify-center text-fg-faint shrink-0">
          {isError ? (
            <span className="text-danger text-xs font-bold">!</span>
          ) : (
            <Spinner size="xs" />
          )}
        </span>
        <div className="flex-1 min-w-0">
          <div className="text-sm text-fg truncate">{pending.name}</div>
          <div className="text-[10px] font-mono text-fg-faint truncate">
            {isError ? pending.error : `${pending.kind}…`}
          </div>
        </div>
        <button
          type="button"
          onClick={onCancel}
          aria-label="削除"
          title={isError ? "削除" : "キャンセル"}
          className="shrink-0 w-6 h-6 inline-flex items-center justify-center rounded text-fg-faint hover:text-danger hover:bg-danger/10"
        >
          <IconX className="w-3 h-3" />
        </button>
      </div>
    </li>
  );
}

function GroupRow({
  group,
  expanded,
  onToggle,
  activeSession,
  onLaunch,
  onOpenSession,
  onStopSession,
  onDeleteProject,
}: {
  group: Group;
  expanded: boolean;
  onToggle: () => void;
  activeSession: string | null;
  onLaunch: (() => void) | null;
  onOpenSession: (name: string) => void;
  onStopSession: (s: TmuxSession) => void;
  onDeleteProject?: (name: string) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const name = group.project?.name ?? "未紐付け";
  const path = group.project?.path ?? "";
  const isGit = group.project?.isGit ?? false;
  const sessions = group.sessions;

  return (
    <li className="px-1.5 relative">
      <div
        className={[
          "group flex items-center gap-1 px-1.5 py-1.5 rounded-md",
          "hover:bg-surface-muted",
        ].join(" ")}
        onContextMenu={(e) => {
          if (!group.project || !onDeleteProject) return;
          e.preventDefault();
          setMenuOpen((v) => !v);
        }}
      >
        <button
          type="button"
          onClick={onToggle}
          className="shrink-0 w-5 h-5 inline-flex items-center justify-center text-fg-faint hover:text-fg"
          aria-label={expanded ? "閉じる" : "開く"}
        >
          {expanded ? (
            <IconChevronDown className="w-3.5 h-3.5" />
          ) : (
            <IconChevronRight className="w-3.5 h-3.5" />
          )}
        </button>
        <button
          type="button"
          onClick={onToggle}
          className="flex-1 min-w-0 text-left flex items-center gap-1.5"
        >
          {group.project ? (
            <IconFolder className="w-3.5 h-3.5 text-fg-faint shrink-0" />
          ) : (
            <IconTerminal className="w-3.5 h-3.5 text-fg-faint shrink-0" />
          )}
          <span className="text-sm text-fg truncate">{name}</span>
          {sessions.length > 0 && (
            <span className="text-[10px] tabular-nums text-fg-faint shrink-0">
              {sessions.length}
            </span>
          )}
          {isGit && (
            <Badge tone="emerald" className="ml-0.5 shrink-0">
              git
            </Badge>
          )}
        </button>
        {group.project && onDeleteProject && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setMenuOpen((v) => !v);
            }}
            aria-label="プロジェクト操作"
            title="…"
            className="shrink-0 w-6 h-6 inline-flex items-center justify-center rounded text-fg-faint hover:text-fg hover:bg-surface-elev opacity-0 group-hover:opacity-100 transition-opacity"
          >
            <span className="text-xs">⋯</span>
          </button>
        )}
        {onLaunch && (
          <button
            type="button"
            onClick={onLaunch}
            aria-label={`${name} で新規セッション`}
            title="新規セッション起動"
            className="shrink-0 w-6 h-6 inline-flex items-center justify-center rounded text-fg-faint hover:text-fg hover:bg-surface-elev"
          >
            <IconPlus className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
      {menuOpen && group.project && onDeleteProject && (
        <>
          <div className="fixed inset-0 z-20" onClick={() => setMenuOpen(false)} aria-hidden />
          <div className="absolute right-2 top-8 z-30 w-44 bg-surface-elev border border-line rounded-md shadow-lg py-1">
            <button
              type="button"
              onClick={() => {
                setMenuOpen(false);
                onDeleteProject(group.project!.name);
              }}
              className="w-full text-left px-3 py-1.5 text-xs text-danger hover:bg-danger/5"
            >
              プロジェクトを削除
            </button>
          </div>
        </>
      )}
      {expanded && (
        <>
          {path && (
            <div className="px-2.5 -mt-1 mb-1 ml-4 text-[10px] font-mono text-fg-faint truncate">
              {path}
            </div>
          )}
          {sessions.length === 0 ? (
            onLaunch && (
              <button
                type="button"
                onClick={onLaunch}
                className="ml-6 mb-1 px-2 py-1 text-[11px] text-fg-subtle hover:text-fg inline-flex items-center gap-1.5"
              >
                <IconPlus className="w-3 h-3" /> 新規セッション
              </button>
            )
          ) : (
            <ul className="ml-1.5 mb-1">
              {sessions.map((s) => (
                <SessionRow
                  key={s.name}
                  session={s}
                  active={activeSession === s.name}
                  onOpen={() => onOpenSession(s.name)}
                  onStop={() => onStopSession(s)}
                />
              ))}
            </ul>
          )}
        </>
      )}
    </li>
  );
}

function SessionRow({
  session,
  active,
  onOpen,
  onStop,
}: {
  session: TmuxSession;
  active: boolean;
  onOpen: () => void;
  onStop: () => void;
}) {
  const hasWorktree = !!session.worktreePath;
  const cliLabel = CLI_LABEL[session.cli] || session.cli || "claude";
  return (
    <li
      className={[
        "group ml-4 flex items-start gap-1.5 px-1.5 py-1.5 rounded-md",
        "cursor-pointer",
        active ? "bg-accent/10 text-fg" : "hover:bg-surface-muted text-fg-muted",
      ].join(" ")}
      onClick={onOpen}
    >
      <span
        aria-hidden
        className={[
          "mt-1.5 inline-block w-1.5 h-1.5 rounded-full shrink-0",
          session.attached ? "bg-success" : "bg-fg-faint",
        ].join(" ")}
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          {hasWorktree ? (
            <IconBranch className="w-3 h-3 text-fg-faint shrink-0" />
          ) : (
            <IconTerminal className="w-3 h-3 text-fg-faint shrink-0" />
          )}
          <span className="text-[12px] font-mono truncate">{session.name}</span>
        </div>
        <div className="mt-0.5 flex items-center gap-1.5 text-[10px] text-fg-faint">
          <span>{cliLabel}</span>
          <span>·</span>
          <span>{formatRelative(session.createdAt)}</span>
        </div>
      </div>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onStop();
        }}
        aria-label="停止"
        title="停止"
        className="shrink-0 w-5 h-5 inline-flex items-center justify-center rounded text-fg-faint hover:text-danger hover:bg-danger/10"
      >
        <IconX className="w-3 h-3" />
      </button>
    </li>
  );
}

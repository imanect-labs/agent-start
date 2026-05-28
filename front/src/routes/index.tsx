import useSWR, { mutate } from "swr";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type ImperativePanelHandle,
  Panel,
  PanelGroup,
  PanelResizeHandle,
} from "react-resizable-panels";
import { useToast } from "@/components/Toast";
import {
  Sidebar,
  sessionProjectPath,
  type PendingProject,
  type Project,
  type TmuxSession,
} from "@/components/Sidebar";
import { MainPane, type RecentProject } from "@/components/MainPane";
import { RightPane } from "@/components/RightPane";
import { LaunchConfirmSheet, type LaunchOverrides } from "@/components/LaunchConfirmSheet";
import { IssuesSheet } from "@/components/IssuesSheet";
import { DeleteConfirmSheet, type DeleteTarget } from "@/components/DeleteConfirmSheet";
import { AddProjectModal } from "@/components/AddProjectModal";
import { DeleteProjectConfirm } from "@/components/DeleteProjectConfirm";
import { makeTabId, type DiffMode, type SessionTabs, type Tab } from "@/components/tab-types";
import { useMediaQuery } from "@/lib/useMediaQuery";

const fetcher = (url: string) => fetch(url).then((r) => r.json());

const STORAGE_KEY = "agent-start:tabs:v1";

type PendingSession = {
  /** Temporary client id ("pending:…"); used as the session name until the
   *  host returns the real one. */
  tempId: string;
  /** Project path, for grouping in the sidebar. */
  projectPath: string;
  cli: string;
  createWorktree: boolean;
  createdAt: number;
};

function makePendingId(): string {
  return `pending:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

type StoredTabs = {
  perSession: Record<string, SessionTabs>;
  activeSession: string | null;
  rightPaneOpen: boolean;
};

function loadStored(): StoredTabs {
  if (typeof window === "undefined")
    return { perSession: {}, activeSession: null, rightPaneOpen: true };
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) throw new Error("missing");
    const parsed = JSON.parse(raw) as StoredTabs;
    return {
      perSession: parsed.perSession ?? {},
      activeSession: parsed.activeSession ?? null,
      rightPaneOpen: parsed.rightPaneOpen ?? true,
    };
  } catch {
    return { perSession: {}, activeSession: null, rightPaneOpen: true };
  }
}

function persistStored(s: StoredTabs) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore
  }
}

function ResizeHandle() {
  return (
    <PanelResizeHandle
      className={[
        "group relative w-1.5 shrink-0",
        "bg-transparent data-[resize-handle-active]:bg-accent/40",
        "hover:bg-accent/30 transition-colors",
        "cursor-col-resize",
      ].join(" ")}
    >
      <div className="absolute inset-y-0 left-1/2 -translate-x-1/2 w-px bg-line group-hover:bg-accent/60 group-data-[resize-handle-active]:bg-accent" />
    </PanelResizeHandle>
  );
}

export function IndexPage() {
  const toast = useToast();

  const [launchTarget, setLaunchTarget] = useState<Project | null>(null);
  const [issuesTarget, setIssuesTarget] = useState<Project | null>(null);
  // When launching from an issue, the issue context to forward as the
  // agent's initial prompt (and to show in the launch sheet banner).
  const [pendingIssue, setPendingIssue] = useState<{
    prompt: string;
    number: number;
    title: string;
  } | null>(null);
  // Optimistic placeholders for sessions whose POST /api/sessions is still in
  // flight. The host assigns the real name, so we track these by a temporary
  // id and reconcile when the response arrives.
  const [pendingSessions, setPendingSessions] = useState<PendingSession[]>([]);
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [projectToDelete, setProjectToDelete] = useState<string | null>(null);

  // Per-session tab state. `perSession[sessionName]` is undefined until the
  // user has opened the session for the first time.
  //
  // Hydration runs via the useState initializer (not an effect) so the
  // very first render already has the saved tabs. An effect-based
  // hydrate races with the persist effect on the same mount: both fire
  // in the same commit, persist writes the empty initial state, and on
  // the next reload the saved tabs are gone — exactly the "terminal1
  // 以外の全てのタブが消える" symptom users hit.
  const initial = useMemo(loadStored, []);
  const [perSession, setPerSession] = useState<Record<string, SessionTabs>>(
    () => initial.perSession,
  );
  const [activeSession, setActiveSession] = useState<string | null>(() => initial.activeSession);
  const [rightPaneOpen, setRightPaneOpen] = useState(() => initial.rightPaneOpen);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [restarting, setRestarting] = useState<string | null>(null);

  // Persist on every change. Safe to fire on mount now that the
  // initial state already equals what's in localStorage.
  useEffect(() => {
    persistStored({ perSession, activeSession, rightPaneOpen });
  }, [perSession, activeSession, rightPaneOpen]);

  const { data: projData, isLoading: projLoading } = useSWR<{
    projects: Project[];
    pending?: PendingProject[];
  }>("/api/projects", fetcher, {
    refreshInterval: (data) => ((data?.pending?.length ?? 0) > 0 ? 2000 : 0),
  });

  const { data: sessData, isLoading: sessLoading } = useSWR<{
    sessions: TmuxSession[];
  }>("/api/sessions", fetcher, { refreshInterval: 5000 });

  // Config tells us which CLIs run in chat mode (#34) and the model menu.
  const { data: configData } = useSWR<{
    clis: { key: string; mode?: string }[];
    chat?: { models?: { id: string; label: string }[]; defaultModel?: string | null };
  }>("/api/config", fetcher);

  const projects = projData?.projects ?? [];
  const pendingProjects = projData?.pending ?? [];
  const realSessions = sessData?.sessions ?? [];

  // Merge optimistic placeholders into the session list so the sidebar and
  // main pane reflect a launch the instant the user confirms it, before the
  // host has finished creating the PTY (and worktree).
  const sessions = useMemo<TmuxSession[]>(() => {
    if (pendingSessions.length === 0) return realSessions;
    const placeholders: TmuxSession[] = pendingSessions.map((p) => ({
      name: p.tempId,
      path: p.projectPath,
      createdAt: p.createdAt,
      attached: false,
      cli: p.cli,
      worktreePath: "",
      origPath: "",
      pending: true,
    }));
    return [...placeholders, ...realSessions];
  }, [realSessions, pendingSessions]);

  const chatClis = useMemo(
    () => new Set((configData?.clis ?? []).filter((c) => c.mode === "chat").map((c) => c.key)),
    [configData],
  );
  const chatModels = configData?.chat?.models ?? [];
  const chatDefaultModel = configData?.chat?.defaultModel ?? null;

  // "Recent projects" for the welcome screen (#86): bubble up the projects
  // whose most-recent session was launched most recently. Pending placeholders
  // are skipped — they would otherwise jump to the top while the host is
  // still creating the session.
  const recentProjects = useMemo<RecentProject[]>(() => {
    const projByPath = new Map(projects.map((p) => [p.path, p]));
    const lastByPath = new Map<string, { name: string; at: number }>();
    for (const s of sessions) {
      if (s.pending) continue;
      const pp = sessionProjectPath(s);
      const cur = lastByPath.get(pp);
      if (!cur || s.createdAt > cur.at) {
        lastByPath.set(pp, { name: s.name, at: s.createdAt });
      }
    }
    return Array.from(lastByPath.entries())
      .map(([path, last]): RecentProject | null => {
        const project = projByPath.get(path);
        if (!project) return null;
        return { project, lastSessionName: last.name, lastSessionAt: last.at };
      })
      .filter((r): r is RecentProject => r !== null)
      .sort((a, b) => b.lastSessionAt - a.lastSessionAt)
      .slice(0, 6);
  }, [projects, sessions]);

  // Refs so the stable `openSession` callback can read the latest values
  // without resubscribing (it is created once with an empty dep list).
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const chatClisRef = useRef(chatClis);
  chatClisRef.current = chatClis;

  // Note: we deliberately do not auto-prune perSession against
  // /api/sessions. Both delete flows (handleStopConfirm, manual close)
  // already drop the entry explicitly, and the host now rehydrates
  // stopped sessions on restart so the list shouldn't shrink
  // unexpectedly. Auto-pruning races with restarts and wipes the
  // user's open tabs irreversibly through the localStorage persist.

  const openSession = useCallback((name: string, cliHint?: string) => {
    setActiveSession(name);
    // Pending placeholders have no real PTY/tabs yet — the main pane renders a
    // skeleton from the session's `pending` flag. Don't seed a tab entry that
    // would orphan in localStorage once the real session takes over.
    if (name.startsWith("pending:")) return;
    setPerSession((prev) => {
      if (prev[name]) return prev;
      // First open: the primary tab depends on the session's CLI mode.
      // A chat-mode CLI (#34, decision 11) opens a ChatTab with no PTY;
      // everything else opens a terminal on window 0. For stopped
      // sessions the server replays saved state over the WS.
      const cli = cliHint ?? sessionsRef.current.find((s) => s.name === name)?.cli;
      const isChat = cli ? chatClisRef.current.has(cli) : false;
      const id = makeTabId();
      const tab: Tab = isChat ? { id, kind: "chat" } : { id, kind: "terminal", windowId: 0 };
      return { ...prev, [name]: { tabs: [tab], activeTabId: id } };
    });
  }, []);

  const selectTab = useCallback(
    (tabId: string) => {
      if (!activeSession) return;
      setPerSession((prev) => {
        const cur = prev[activeSession];
        if (!cur) return prev;
        if (cur.activeTabId === tabId) return prev;
        return { ...prev, [activeSession]: { ...cur, activeTabId: tabId } };
      });
    },
    [activeSession],
  );

  const addTerminalTab = useCallback(async () => {
    if (!activeSession) return;
    const sessionName = activeSession;
    // Optimistically add a pending terminal tab (skeleton) and focus it; the
    // real tmux window index is filled in once the host creates the window.
    const id = makeTabId();
    setPerSession((prev) => {
      const cur = prev[sessionName];
      if (!cur) return prev;
      const tab: Tab = { id, kind: "terminal", windowId: -1, pending: true };
      return {
        ...prev,
        [sessionName]: { tabs: [...cur.tabs, tab], activeTabId: id },
      };
    });
    try {
      const res = await fetch(`/api/sessions/${encodeURIComponent(sessionName)}/windows`, {
        method: "POST",
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
      // Bind the placeholder to the real window — the Terminal key includes
      // windowId so it mounts onto the live PTY here.
      setPerSession((prev) => {
        const cur = prev[sessionName];
        if (!cur) return prev;
        const nextTabs = cur.tabs.map((t) =>
          t.id === id ? ({ ...t, windowId: json.index, pending: false } as Tab) : t,
        );
        return { ...prev, [sessionName]: { ...cur, tabs: nextTabs } };
      });
    } catch (e) {
      // Drop the placeholder tab on failure, restoring focus to a sibling.
      setPerSession((prev) => {
        const cur = prev[sessionName];
        if (!cur) return prev;
        const idx = cur.tabs.findIndex((t) => t.id === id);
        if (idx === -1) return prev;
        const nextTabs = cur.tabs.filter((t) => t.id !== id);
        if (nextTabs.length === 0) return prev;
        const nextActive =
          cur.activeTabId === id
            ? nextTabs[Math.min(idx, nextTabs.length - 1)].id
            : cur.activeTabId;
        return { ...prev, [sessionName]: { tabs: nextTabs, activeTabId: nextActive } };
      });
      toast({
        title: "ターミナルタブ追加失敗",
        description: (e as Error).message,
        color: "danger",
      });
    }
  }, [activeSession, toast]);

  const updateTab = useCallback(
    (tabId: string, patch: Partial<Tab>) => {
      if (!activeSession) return;
      setPerSession((prev) => {
        const cur = prev[activeSession];
        if (!cur) return prev;
        const nextTabs = cur.tabs.map((t) => (t.id === tabId ? ({ ...t, ...patch } as Tab) : t));
        return { ...prev, [activeSession]: { ...cur, tabs: nextTabs } };
      });
    },
    [activeSession],
  );

  const openEditorTab = useCallback(
    (path: string) => {
      if (!activeSession) return;
      setPerSession((prev) => {
        const cur = prev[activeSession];
        if (!cur) return prev;
        const existing = cur.tabs.find((t) => t.kind === "editor" && t.path === path);
        if (existing) {
          return { ...prev, [activeSession]: { ...cur, activeTabId: existing.id } };
        }
        const id = makeTabId();
        const tab: Tab = { id, kind: "editor", path, view: "edit" };
        return {
          ...prev,
          [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id },
        };
      });
    },
    [activeSession],
  );

  const openDiffTab = useCallback(
    (cwd: string, file: string, mode: DiffMode) => {
      if (!activeSession) return;
      setPerSession((prev) => {
        const cur = prev[activeSession];
        if (!cur) return prev;
        const existing = cur.tabs.find(
          (t) => t.kind === "diff" && t.file === file && t.mode === mode && t.cwd === cwd,
        );
        if (existing) {
          return { ...prev, [activeSession]: { ...cur, activeTabId: existing.id } };
        }
        const id = makeTabId();
        const tab: Tab = { id, kind: "diff", cwd, file, mode };
        return {
          ...prev,
          [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id },
        };
      });
    },
    [activeSession],
  );

  const addGuiTab = useCallback(async () => {
    if (!activeSession) return;

    // Reserve the popup synchronously from the click handler so transient
    // user activation is still alive when `window.open` runs — Safari and
    // Firefox treat any `window.open` *after* an `await` as not
    // user-initiated and silently block it. We close the reservation
    // below if the preference turns out to be embedded mode.
    const reservedPopup = window.open("about:blank", "_blank");

    let openInNewTab = false;
    try {
      const r = await fetch("/api/preferences");
      if (r.ok) {
        const j = (await r.json()) as { preferences?: { guiOpenInNewTab?: boolean } };
        openInNewTab = !!j.preferences?.guiOpenInNewTab;
      }
    } catch {
      // Fall back to embedded mode on any preferences fetch error.
    }

    if (openInNewTab) {
      const popup = reservedPopup;
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(activeSession)}/novnc`, {
          method: "POST",
        });
        if (!res.ok) {
          const body = (await res.json().catch(() => ({}))) as { error?: string };
          throw new Error(body?.error || `HTTP ${res.status}`);
        }
        const data = (await res.json()) as { url: string };
        if (popup) {
          popup.location.href = data.url;
        } else {
          window.location.href = data.url;
        }
      } catch (e) {
        if (popup) popup.close();
        toast({
          title: "GUI を開けませんでした",
          description: e instanceof Error ? e.message : String(e),
          color: "danger",
        });
      }
      return;
    }

    // Embedded mode — release the reservation.
    if (reservedPopup) reservedPopup.close();

    const id = makeTabId();
    setPerSession((prev) => {
      const cur = prev[activeSession];
      if (!cur) return prev;
      // Single-instance: focus the existing GUI tab if one is already open.
      const existing = cur.tabs.find((t) => t.kind === "gui");
      if (existing) {
        return {
          ...prev,
          [activeSession]: { ...cur, activeTabId: existing.id },
        };
      }
      const tab: Tab = { id, kind: "gui" };
      return {
        ...prev,
        [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id },
      };
    });
  }, [activeSession, toast]);

  const addFilesTab = useCallback(() => {
    if (!activeSession) return;
    const id = makeTabId();
    setPerSession((prev) => {
      const cur = prev[activeSession];
      if (!cur) return prev;
      // If a files tab already exists, just focus it (single instance).
      const existing = cur.tabs.find((t) => t.kind === "files");
      if (existing) {
        return {
          ...prev,
          [activeSession]: { ...cur, activeTabId: existing.id },
        };
      }
      const tab: Tab = { id, kind: "files" };
      return {
        ...prev,
        [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id },
      };
    });
  }, [activeSession]);

  const closeTab = useCallback(
    async (tabId: string) => {
      if (!activeSession) return;
      const cur = perSession[activeSession];
      if (!cur) return;
      const tab = cur.tabs.find((t) => t.id === tabId);
      if (!tab) return;
      // If terminal tab on window > 0, also kill the tmux window. Window 0 is
      // tied to the session itself — closing that tab just removes it from
      // the UI but keeps the session running.
      if (tab.kind === "terminal" && tab.windowId > 0) {
        try {
          await fetch(
            `/api/sessions/${encodeURIComponent(activeSession)}/windows/${tab.windowId}`,
            { method: "DELETE" },
          );
        } catch {
          // non-fatal
        }
      }
      setPerSession((prev) => {
        const c = prev[activeSession];
        if (!c) return prev;
        const idx = c.tabs.findIndex((t) => t.id === tabId);
        if (idx === -1) return prev;
        const nextTabs = c.tabs.filter((t) => t.id !== tabId);
        if (nextTabs.length === 0) {
          // No tabs left — remove session entry entirely; user can reopen it
          // from the sidebar to get a fresh terminal tab.
          const next = { ...prev };
          delete next[activeSession];
          return next;
        }
        const wasActive = c.activeTabId === tabId;
        const nextActive = wasActive
          ? nextTabs[Math.min(idx, nextTabs.length - 1)].id
          : c.activeTabId;
        return {
          ...prev,
          [activeSession]: { tabs: nextTabs, activeTabId: nextActive },
        };
      });
    },
    [activeSession, perSession],
  );

  const handleLaunch = async (o: LaunchOverrides) => {
    if (!launchTarget) return;
    const projectPath = launchTarget.path;
    // Optimistically insert a placeholder and switch to it immediately: the
    // sidebar shows a spinner row and the main pane a terminal skeleton while
    // the host creates the PTY (and, slowest, the worktree).
    const tempId = makePendingId();
    setPendingSessions((prev) => [
      ...prev,
      {
        tempId,
        projectPath,
        cli: o.cli,
        createWorktree: o.createWorktree,
        createdAt: Date.now(),
      },
    ]);
    setActiveSession(tempId);
    setLaunchTarget(null);
    setPendingIssue(null);
    const issuePrompt = pendingIssue?.prompt;
    try {
      const res = await fetch("/api/sessions", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          projectPath,
          cli: o.cli,
          skipPermissions: o.skipPermissions,
          extraArgs: o.extraArgs,
          createWorktree: o.createWorktree,
          prompt: issuePrompt,
        }),
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      toast({
        title: "起動しました",
        description: json.name,
        color: "success",
      });
      // Pull in the real session, then switch the active session to it before
      // dropping the placeholder so the main pane never flashes the welcome
      // screen during the swap.
      await mutate("/api/sessions");
      if (typeof json.name === "string") openSession(json.name, json.cli);
      setPendingSessions((prev) => prev.filter((p) => p.tempId !== tempId));
    } catch (e) {
      // Roll back the placeholder and leave the user where they were.
      setPendingSessions((prev) => prev.filter((p) => p.tempId !== tempId));
      setActiveSession((cur) => (cur === tempId ? null : cur));
      toast({
        title: "起動失敗",
        description: (e as Error).message,
        color: "danger",
      });
    }
  };

  const handleRestartSession = useCallback(
    async (name: string) => {
      setRestarting(name);
      // Optimistically flip `stopped` → live so the terminal remounts and
      // shows its own "接続中…" overlay right away. The Terminal tab key
      // includes the stopped flag, so this remounts onto the (soon) fresh PTY;
      // if the WS races ahead of the PTY it auto-retries with backoff.
      mutate(
        "/api/sessions",
        (cur?: { sessions: TmuxSession[] }) => ({
          sessions: (cur?.sessions ?? []).map((s) =>
            s.name === name ? { ...s, stopped: false } : s,
          ),
        }),
        { revalidate: false },
      );
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(name)}/restart`, {
          method: "POST",
        });
        const json = await res.json();
        if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
        toast({ title: "再開しました", description: name, color: "success" });
        // Sync with the server's authoritative state.
        await mutate("/api/sessions");
      } catch (e) {
        toast({
          title: "再開失敗",
          description: (e as Error).message,
          color: "danger",
        });
        // Roll back the optimistic flip to the server's real state.
        mutate("/api/sessions");
      } finally {
        setRestarting(null);
      }
    },
    [toast],
  );

  const handleStopConfirm = async (deleteWorktree: boolean) => {
    if (!deleteTarget) return;
    const targetName = deleteTarget.name;
    setDeleting(true);
    // Optimistically drop the session from the list so the sidebar reflects
    // the removal immediately instead of waiting for the next poll.
    mutate(
      "/api/sessions",
      (cur?: { sessions: TmuxSession[] }) => ({
        sessions: (cur?.sessions ?? []).filter((s) => s.name !== targetName),
      }),
      { revalidate: false },
    );
    try {
      const url = `/api/sessions/${encodeURIComponent(deleteTarget.name)}${
        deleteWorktree ? "?deleteWorktree=1" : ""
      }`;
      const res = await fetch(url, { method: "DELETE" });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      if (json.worktreeError) {
        toast({
          title: "停止 (worktree 削除失敗)",
          description: json.worktreeError,
          color: "warning",
        });
      } else {
        toast({ title: "停止しました", color: "success" });
      }
      // Remove tab state for this session
      setPerSession((prev) => {
        if (!prev[deleteTarget.name]) return prev;
        const next = { ...prev };
        delete next[deleteTarget.name];
        return next;
      });
      setActiveSession((cur) => (cur === deleteTarget.name ? null : cur));
      setDeleteTarget(null);
      mutate("/api/sessions");
    } catch (e) {
      toast({
        title: "停止失敗",
        description: (e as Error).message,
        color: "danger",
      });
      // Restore the optimistically removed session on failure.
      mutate("/api/sessions");
    } finally {
      setDeleting(false);
    }
  };

  const refresh = () => {
    mutate("/api/sessions");
    mutate("/api/projects");
  };

  const activeSessionObj = useMemo(
    () => sessions.find((s) => s.name === activeSession) ?? null,
    [sessions, activeSession],
  );
  const activeTabs = activeSession ? perSession[activeSession] : null;
  const activeCwd =
    activeSessionObj?.worktreePath || activeSessionObj?.path || activeSessionObj?.origPath || "";

  // Graph / Tree tabs snapshot the cwd at open time (like diff tabs) and
  // are single-instance per session.
  const addGraphTab = useCallback(() => {
    if (!activeSession || !activeCwd) return;
    setPerSession((prev) => {
      const cur = prev[activeSession];
      if (!cur) return prev;
      const existing = cur.tabs.find((t) => t.kind === "graph");
      if (existing) return { ...prev, [activeSession]: { ...cur, activeTabId: existing.id } };
      const id = makeTabId();
      const tab: Tab = { id, kind: "graph", cwd: activeCwd };
      return { ...prev, [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id } };
    });
  }, [activeSession, activeCwd]);

  const addTreeTab = useCallback(() => {
    if (!activeSession || !activeCwd) return;
    setPerSession((prev) => {
      const cur = prev[activeSession];
      if (!cur) return prev;
      const existing = cur.tabs.find((t) => t.kind === "tree");
      if (existing) return { ...prev, [activeSession]: { ...cur, activeTabId: existing.id } };
      const id = makeTabId();
      const tab: Tab = { id, kind: "tree", cwd: activeCwd };
      return { ...prev, [activeSession]: { tabs: [...cur.tabs, tab], activeTabId: id } };
    });
  }, [activeSession, activeCwd]);

  const isDesktop = useMediaQuery("(min-width: 1024px)");
  const rightPanelRef = useRef<ImperativePanelHandle>(null);

  // On desktop the right pane lives in a resizable Panel; sync the
  // open flag with the imperative collapse/expand API so the user's
  // dragging the handle and clicking the toggle stay consistent.
  useEffect(() => {
    if (!isDesktop) return;
    const panel = rightPanelRef.current;
    if (!panel) return;
    if (rightPaneOpen) {
      if (panel.isCollapsed()) panel.expand();
    } else if (!panel.isCollapsed()) {
      panel.collapse();
    }
  }, [isDesktop, rightPaneOpen]);

  const sidebar = (mode: "inline" | "overlay") => (
    <Sidebar
      mode={mode}
      projects={projects}
      pending={pendingProjects}
      onAddProject={() => setAddOpen(true)}
      onDeleteProject={(name) => setProjectToDelete(name)}
      sessions={sessions}
      loadingProjects={projLoading}
      loadingSessions={sessLoading}
      activeSession={activeSession}
      onLaunchProject={(p) => {
        setSidebarOpen(false);
        setLaunchTarget(p);
      }}
      onBrowseIssues={(p) => {
        setSidebarOpen(false);
        setIssuesTarget(p);
      }}
      onOpenSession={(n) => {
        setSidebarOpen(false);
        openSession(n);
      }}
      onStopSession={(s) =>
        setDeleteTarget({
          name: s.name,
          worktreePath: s.worktreePath,
          origPath: s.origPath,
        })
      }
      onRefresh={refresh}
      open={sidebarOpen}
      onClose={() => setSidebarOpen(false)}
    />
  );

  const mainPane = (
    <MainPane
      session={activeSessionObj}
      tabs={activeTabs ?? null}
      onSelectTab={selectTab}
      onCloseTab={closeTab}
      onAddTerminal={addTerminalTab}
      onAddFiles={addFilesTab}
      onAddGui={addGuiTab}
      onAddGraph={addGraphTab}
      onAddTree={addTreeTab}
      onOpenFile={openEditorTab}
      onStopSession={(s) =>
        setDeleteTarget({
          name: s.name,
          worktreePath: s.worktreePath,
          origPath: s.origPath,
        })
      }
      onRestartSession={(s) => handleRestartSession(s.name)}
      restarting={restarting === activeSession}
      rightPaneOpen={rightPaneOpen}
      onToggleRightPane={() => setRightPaneOpen((v) => !v)}
      onUpdateTab={updateTab}
      onToggleSidebar={() => setSidebarOpen((v) => !v)}
      onOpenDiff={(file, mode) => openDiffTab(activeCwd, file, mode)}
      chatModels={chatModels}
      chatDefaultModel={chatDefaultModel}
      recentProjects={recentProjects}
      onLaunchProject={(p) => setLaunchTarget(p)}
      onOpenSession={(n) => openSession(n)}
    />
  );

  const rightPane = (mode: "inline" | "overlay") =>
    activeSessionObj ? (
      <RightPane
        mode={mode}
        cwd={activeCwd}
        onClose={() => setRightPaneOpen(false)}
        onOpenFile={openEditorTab}
        onOpenDiff={(file, mode) => openDiffTab(activeCwd, file, mode)}
      />
    ) : null;

  return (
    <main className="h-[var(--app-h)] flex bg-app text-fg overflow-hidden safe-top safe-bottom safe-left safe-right">
      {isDesktop ? (
        <PanelGroup
          direction="horizontal"
          autoSaveId="agent-start:layout:v1"
          className="flex-1 min-w-0"
        >
          <Panel
            id="sidebar"
            order={1}
            defaultSize={18}
            minSize={12}
            maxSize={35}
            className="flex flex-col"
          >
            {sidebar("inline")}
          </Panel>
          <ResizeHandle />
          <Panel id="main" order={2} minSize={30} className="flex flex-col min-w-0">
            {mainPane}
          </Panel>
          <ResizeHandle />
          <Panel
            id="right"
            order={3}
            ref={rightPanelRef}
            defaultSize={22}
            minSize={16}
            maxSize={45}
            collapsible
            collapsedSize={0}
            onCollapse={() => setRightPaneOpen(false)}
            onExpand={() => setRightPaneOpen(true)}
            className="flex flex-col"
          >
            {activeSessionObj ? rightPane("inline") : null}
          </Panel>
        </PanelGroup>
      ) : (
        <>
          {sidebar("overlay")}
          <div className="flex-1 min-w-0 flex">{mainPane}</div>
          {rightPaneOpen && rightPane("overlay")}
        </>
      )}

      <IssuesSheet
        isOpen={!!issuesTarget}
        projectName={issuesTarget?.name ?? ""}
        projectPath={issuesTarget?.path ?? ""}
        onClose={() => setIssuesTarget(null)}
        onLaunchIssue={(prompt, number, title) => {
          const project = issuesTarget;
          setIssuesTarget(null);
          setPendingIssue({ prompt, number, title });
          setLaunchTarget(project);
        }}
      />

      <LaunchConfirmSheet
        isOpen={!!launchTarget}
        projectName={launchTarget?.name ?? ""}
        projectPath={launchTarget?.path ?? ""}
        isGit={!!launchTarget?.isGit}
        issueContext={
          pendingIssue ? { number: pendingIssue.number, title: pendingIssue.title } : undefined
        }
        onClose={() => {
          setLaunchTarget(null);
          setPendingIssue(null);
        }}
        onLaunch={handleLaunch}
      />

      <DeleteConfirmSheet
        target={deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleStopConfirm}
        busy={deleting}
      />

      <AddProjectModal open={addOpen} onClose={() => setAddOpen(false)} />
      <DeleteProjectConfirm
        open={!!projectToDelete}
        name={projectToDelete ?? ""}
        onClose={() => setProjectToDelete(null)}
      />
    </main>
  );
}

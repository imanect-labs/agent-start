import useSWR, { mutate } from "swr";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useToast } from "@/components/Toast";
import { Sidebar, type PendingProject, type Project, type TmuxSession } from "@/components/Sidebar";
import { MainPane } from "@/components/MainPane";
import { RightPane } from "@/components/RightPane";
import { LaunchConfirmSheet, type LaunchOverrides } from "@/components/LaunchConfirmSheet";
import { DeleteConfirmSheet, type DeleteTarget } from "@/components/DeleteConfirmSheet";
import { AddProjectModal } from "@/components/AddProjectModal";
import { DeleteProjectConfirm } from "@/components/DeleteProjectConfirm";
import { makeTabId, type DiffMode, type SessionTabs, type Tab } from "@/components/tab-types";

const fetcher = (url: string) => fetch(url).then((r) => r.json());

const STORAGE_KEY = "agent-start:tabs:v1";

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

export function IndexPage() {
  const toast = useToast();

  const [launchTarget, setLaunchTarget] = useState<Project | null>(null);
  const [launching, setLaunching] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [projectToDelete, setProjectToDelete] = useState<string | null>(null);

  // Per-session tab state. `perSession[sessionName]` is undefined until the
  // user has opened the session for the first time.
  const [perSession, setPerSession] = useState<Record<string, SessionTabs>>({});
  const [activeSession, setActiveSession] = useState<string | null>(null);
  const [rightPaneOpen, setRightPaneOpen] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  // Hydrate from localStorage on first mount.
  useEffect(() => {
    const s = loadStored();
    setPerSession(s.perSession);
    setActiveSession(s.activeSession);
    setRightPaneOpen(s.rightPaneOpen);
  }, []);

  // Persist on every change.
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

  const projects = projData?.projects ?? [];
  const pendingProjects = projData?.pending ?? [];
  const sessions = sessData?.sessions ?? [];

  // Prune sessions that no longer exist server-side.
  useEffect(() => {
    if (!sessData) return;
    const live = new Set(sessions.map((s) => s.name));
    setPerSession((prev) => {
      const next: Record<string, SessionTabs> = {};
      let changed = false;
      for (const [k, v] of Object.entries(prev)) {
        if (live.has(k)) next[k] = v;
        else changed = true;
      }
      if (!changed) return prev;
      return next;
    });
    setActiveSession((cur) => (cur && live.has(cur) ? cur : null));
  }, [sessData, sessions]);

  const openSession = useCallback((name: string) => {
    setActiveSession(name);
    setPerSession((prev) => {
      if (prev[name]) return prev;
      // First open: terminal tab on window 0. For stopped sessions the
      // server replays the saved scrollback over the WS then closes —
      // the user sees their last terminal state, just can't type.
      const id = makeTabId();
      const tab: Tab = { id, kind: "terminal", windowId: 0 };
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
    try {
      const res = await fetch(`/api/sessions/${encodeURIComponent(activeSession)}/windows`, {
        method: "POST",
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
      const id = makeTabId();
      setPerSession((prev) => {
        const cur = prev[activeSession];
        if (!cur) return prev;
        const tab: Tab = { id, kind: "terminal", windowId: json.index };
        return {
          ...prev,
          [activeSession]: {
            tabs: [...cur.tabs, tab],
            activeTabId: id,
          },
        };
      });
    } catch (e) {
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
    setLaunching(true);
    try {
      const res = await fetch("/api/sessions", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          projectPath: launchTarget.path,
          cli: o.cli,
          skipPermissions: o.skipPermissions,
          extraArgs: o.extraArgs,
          createWorktree: o.createWorktree,
        }),
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      toast({
        title: "起動しました",
        description: json.name,
        color: "success",
      });
      setLaunchTarget(null);
      await mutate("/api/sessions");
      if (typeof json.name === "string") openSession(json.name);
    } catch (e) {
      toast({
        title: "起動失敗",
        description: (e as Error).message,
        color: "danger",
      });
    } finally {
      setLaunching(false);
    }
  };

  const handleStopConfirm = async (deleteWorktree: boolean) => {
    if (!deleteTarget) return;
    setDeleting(true);
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

  return (
    <main className="h-[100dvh] flex bg-app text-fg overflow-hidden">
      <Sidebar
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

      <MainPane
        session={activeSessionObj}
        tabs={activeTabs ?? null}
        onSelectTab={selectTab}
        onCloseTab={closeTab}
        onAddTerminal={addTerminalTab}
        onAddFiles={addFilesTab}
        onStopSession={(s) =>
          setDeleteTarget({
            name: s.name,
            worktreePath: s.worktreePath,
            origPath: s.origPath,
          })
        }
        rightPaneOpen={rightPaneOpen}
        onToggleRightPane={() => setRightPaneOpen((v) => !v)}
        onUpdateTab={updateTab}
        onToggleSidebar={() => setSidebarOpen((v) => !v)}
        onOpenDiff={(file, mode) => openDiffTab(activeCwd, file, mode)}
      />

      {rightPaneOpen && activeSessionObj && (
        <RightPane
          cwd={activeCwd}
          onClose={() => setRightPaneOpen(false)}
          onOpenFile={openEditorTab}
          onOpenDiff={(file, mode) => openDiffTab(activeCwd, file, mode)}
        />
      )}

      <LaunchConfirmSheet
        isOpen={!!launchTarget}
        projectName={launchTarget?.name ?? ""}
        projectPath={launchTarget?.path ?? ""}
        isGit={!!launchTarget?.isGit}
        onClose={() => setLaunchTarget(null)}
        onLaunch={handleLaunch}
        launching={launching}
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

import useSWR, { mutate } from "swr";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type ImperativePanelHandle,
  Panel,
  PanelGroup,
  PanelResizeHandle,
} from "react-resizable-panels";
import { useToast } from "@/components/Toast";
import { Sidebar, type PendingProject, type Project, type TmuxSession } from "@/components/Sidebar";
import { MainPane } from "@/components/MainPane";
import { RightPane } from "@/components/RightPane";
import { LaunchConfirmSheet, type LaunchOverrides } from "@/components/LaunchConfirmSheet";
import { DeleteConfirmSheet, type DeleteTarget } from "@/components/DeleteConfirmSheet";
import { AddProjectModal } from "@/components/AddProjectModal";
import { DeleteProjectConfirm } from "@/components/DeleteProjectConfirm";
import { makeTabId, type DiffMode, type SessionTabs, type Tab } from "@/components/tab-types";
import { useMediaQuery } from "@/lib/useMediaQuery";

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
  const [launching, setLaunching] = useState(false);
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

  const projects = projData?.projects ?? [];
  const pendingProjects = projData?.pending ?? [];
  const sessions = sessData?.sessions ?? [];

  // Note: we deliberately do not auto-prune perSession against
  // /api/sessions. Both delete flows (handleStopConfirm, manual close)
  // already drop the entry explicitly, and the host now rehydrates
  // stopped sessions on restart so the list shouldn't shrink
  // unexpectedly. Auto-pruning races with restarts and wipes the
  // user's open tabs irreversibly through the localStorage persist.

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

  const addGuiTab = useCallback(async () => {
    if (!activeSession) return;

    // Pick popup-or-embed up front: must read the preference BEFORE the
    // first `await`, and (when popup) reserve `window.open` synchronously
    // from this user gesture or Safari/Firefox will block it.
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
      const popup = window.open("about:blank", "_blank");
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

  const handleRestartSession = useCallback(
    async (name: string) => {
      setRestarting(name);
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(name)}/restart`, {
          method: "POST",
        });
        const json = await res.json();
        if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
        toast({ title: "再開しました", description: name, color: "success" });
        // Refresh /api/sessions so `stopped` flips to false; the Terminal
        // tab key includes that flag and will remount onto the fresh PTY.
        await mutate("/api/sessions");
      } catch (e) {
        toast({
          title: "再開失敗",
          description: (e as Error).message,
          color: "danger",
        });
      } finally {
        setRestarting(null);
      }
    },
    [toast],
  );

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

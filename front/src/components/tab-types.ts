/**
 * Client-side tab model.
 *
 * Each session has its own list of tabs (UI views). A "terminal" tab is
 * backed by a tmux window in that session — windowId references the tmux
 * window index. A "files" tab is purely client-side and renders the git
 * changes / diff for the session's cwd.
 */
export type TerminalTab = {
  id: string;
  kind: "terminal";
  /** tmux window index this tab is bound to */
  windowId: number;
  /** optional display label; falls back to "Terminal N" */
  label?: string;
};

export type FilesTab = {
  id: string;
  kind: "files";
  label?: string;
};

export type EditorTab = {
  id: string;
  kind: "editor";
  /** Absolute path to the file being edited. */
  path: string;
  /** Inline view mode toggle: Edit / Preview. */
  view: "edit" | "preview";
  /** True while the buffer differs from the on-disk content. */
  dirty?: boolean;
  label?: string;
};

export type DiffMode = "worktree" | "staged" | "head";

export type DiffTab = {
  id: string;
  kind: "diff";
  /** Session cwd at the time the tab was opened. */
  cwd: string;
  /** File path relative to cwd, as reported by git status. */
  file: string;
  mode: DiffMode;
  label?: string;
};

export type GuiTab = {
  id: string;
  kind: "gui";
  label?: string;
};

export type GraphTab = {
  id: string;
  kind: "graph";
  /** Session cwd whose commit graph this tab renders. */
  cwd: string;
  label?: string;
};

export type TreeTab = {
  id: string;
  kind: "tree";
  /** Session cwd whose file tree this tab renders. */
  cwd: string;
  label?: string;
};

export type Tab = TerminalTab | FilesTab | EditorTab | DiffTab | GuiTab | GraphTab | TreeTab;

export type SessionTabs = {
  tabs: Tab[];
  activeTabId: string;
};

let tabIdCounter = 0;
export function makeTabId(): string {
  tabIdCounter += 1;
  return `t${Date.now().toString(36)}-${tabIdCounter}`;
}

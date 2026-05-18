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

export type Tab = TerminalTab | FilesTab;

export type SessionTabs = {
  tabs: Tab[];
  activeTabId: string;
};

let tabIdCounter = 0;
export function makeTabId(): string {
  tabIdCounter += 1;
  return `t${Date.now().toString(36)}-${tabIdCounter}`;
}

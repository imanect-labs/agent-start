import { IconBranch, IconFolder, IconTerminal } from "@/components/icons";
import type { Project } from "@/components/Sidebar";
import { formatRelative } from "@/lib/format";
import { SidebarToggle } from "./SidebarToggle";

export type RecentProject = {
  project: Project;
  lastSessionName?: string;
  lastSessionAt: number;
};

export function WelcomeBanner({
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
      {/* Mobile/tablet entry point to the sidebar before any session exists. */}
      {onToggleSidebar && (
        <div className="lg:hidden flex items-center gap-2 px-3 py-2 border-b border-line bg-surface/80 backdrop-blur-md min-h-[52px]">
          <SidebarToggle onToggle={onToggleSidebar} />
          <span className="text-sm font-semibold tracking-tight text-fg">agent-start</span>
        </div>
      )}
      <div className="flex-1 overflow-y-auto scroll-thin">
        <div className="max-w-2xl mx-auto px-6 py-12">
          {/* Hero — the showcase surface for the new design language. */}
          <div className="relative text-center">
            <div
              aria-hidden
              className="pointer-events-none absolute left-1/2 top-0 -translate-x-1/2 -translate-y-8 h-64 w-[28rem] max-w-full rounded-full opacity-70 blur-3xl"
              style={{
                background: "radial-gradient(closest-side, rgb(var(--accent) / 0.18), transparent)",
              }}
            />
            <div className="relative">
              <div className="mx-auto w-16 h-16 rounded-xl bg-accent-soft border border-accent/20 flex items-center justify-center text-accent-subtle shadow-sm">
                <IconTerminal className="w-7 h-7" />
              </div>
              <h2 className="mt-5 text-2xl font-semibold tracking-tight text-fg">
                セッションが選択されていません
              </h2>
              <p className="mt-2 text-sm text-fg-subtle max-w-md mx-auto leading-relaxed">
                左のサイドバーからプロジェクトを選び、<span className="font-mono">＋</span>{" "}
                で新しいセッションを起動するか、稼働中のセッションをクリックしてターミナルを開きます。
              </p>
              <p className="mt-3 text-2xs text-fg-faint inline-flex items-center gap-1">
                <IconBranch className="inline w-3 h-3" /> = worktree 付き
                <span className="mx-1">·</span>
                選択中のセッションには複数のタブとファイル変更ペインを開けます
              </p>
            </div>
          </div>

          {recentProjects.length > 0 && (
            <section className="mt-12">
              <h3 className="text-2xs font-semibold tracking-wide uppercase text-fg-subtle px-1">
                最近のプロジェクト
              </h3>
              <ul className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-3">
                {recentProjects.map((r) => (
                  <li key={r.project.path}>
                    <button
                      type="button"
                      onClick={() => {
                        if (r.lastSessionName && onOpenSession) onOpenSession(r.lastSessionName);
                        else if (onLaunchProject) onLaunchProject(r.project);
                      }}
                      className="w-full text-left p-3 flex items-start gap-2.5 min-w-0 rounded-lg border border-line bg-surface shadow-sm transition-[box-shadow,border-color,background-color] duration-150 hover:border-line-strong hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
                    >
                      <span className="mt-0.5 shrink-0 inline-flex items-center justify-center w-8 h-8 rounded-lg bg-surface-muted text-fg-subtle">
                        <IconFolder className="w-4 h-4" />
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="text-sm font-medium text-fg truncate">{r.project.name}</div>
                        <div className="text-2xs text-fg-subtle truncate">{r.project.path}</div>
                        <div className="text-2xs text-fg-faint mt-0.5">
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

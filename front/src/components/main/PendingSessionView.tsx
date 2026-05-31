import { Badge, Skeleton, Spinner } from "@/components/ui";
import { SidebarToggle } from "./SidebarToggle";
import { TerminalSkeleton } from "./skeletons";

/** Full-pane placeholder shown while POST /api/sessions is in flight: a header
 *  with the project name and a spinner, a skeleton tab bar, and a
 *  terminal-shaped skeleton body. */
export function PendingSessionView({
  name,
  onToggleSidebar,
}: {
  name: string;
  onToggleSidebar?: () => void;
}) {
  return (
    <div className="h-full w-full min-w-0 flex flex-col bg-app" aria-live="polite" aria-busy="true">
      <div className="flex items-center gap-2 sm:gap-3 px-3 sm:px-4 py-2 border-b border-line bg-surface/80 backdrop-blur-md min-h-[52px]">
        {onToggleSidebar && <SidebarToggle onToggle={onToggleSidebar} />}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <Spinner size="sm" />
            <span className="font-mono text-sm text-fg-muted truncate">{name}</span>
            <Badge tone="amber" dot>
              起動中…
            </Badge>
          </div>
          <div className="text-2xs text-fg-subtle mt-0.5">セッションを準備しています</div>
        </div>
      </div>
      {/* Skeleton tab bar */}
      <div className="flex items-stretch min-w-0 border-b border-line bg-surface-muted">
        <div className="shrink-0 flex items-center gap-2 pl-3 pr-4 py-2.5 sm:py-2 border-r border-line">
          <Skeleton className="w-3.5 h-3.5 rounded-sm" />
          <Skeleton className="w-16 h-3 rounded-sm" />
        </div>
      </div>
      <div className="flex-1 min-h-0 p-3">
        <TerminalSkeleton />
      </div>
    </div>
  );
}

import { Skeleton, Spinner } from "@/components/ui";

/** Terminal-shaped skeleton: a status line plus a large rectangle that mirrors
 *  the xterm pane, used while a PTY/window is being created. */
export function TerminalSkeleton() {
  return (
    <div className="flex flex-col gap-2 h-full min-h-0">
      <div className="flex items-center gap-1.5 text-xs">
        <Spinner size="sm" />
        <span className="text-fg-subtle">起動中…</span>
      </div>
      <Skeleton className="flex-1 min-h-0 rounded-lg" />
    </div>
  );
}

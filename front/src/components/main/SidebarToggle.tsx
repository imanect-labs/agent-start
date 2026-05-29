import { IconButton } from "@/components/ui";

/** Mobile/tablet hamburger that opens the sidebar drawer. Hidden on lg+. */
export function SidebarToggle({ onToggle }: { onToggle: () => void }) {
  return (
    <IconButton
      aria-label="サイドバーを開く"
      onClick={onToggle}
      className="lg:hidden -ml-1 h-10 w-10"
    >
      <svg viewBox="0 0 20 20" fill="currentColor" className="w-4 h-4">
        <path d="M3 5h14v2H3V5Zm0 4h14v2H3V9Zm0 4h14v2H3v-2Z" />
      </svg>
    </IconButton>
  );
}

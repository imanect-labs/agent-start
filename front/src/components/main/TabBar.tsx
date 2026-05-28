import { ReactNode } from "react";
import { Menu, MenuButton, MenuItem, MenuList, Tabs, type TabItem } from "@/components/ui";
import { IconBranch, IconFolder, IconPlus, IconTerminal } from "@/components/icons";
import type { Tab } from "@/components/tab-types";

function glyph(ch: string) {
  return (
    <span className="w-3.5 h-3.5 inline-flex items-center justify-center text-2xs text-fg-faint">
      {ch}
    </span>
  );
}

/** Derive the display label + icon for a tab from its kind. */
function describe(t: Tab): { label: ReactNode; icon: ReactNode } {
  const iconCls = "w-3.5 h-3.5 shrink-0 text-fg-faint";
  if (t.kind === "terminal") {
    // Window 0 displays as "Terminal 1".
    return {
      label: t.label ?? `Terminal ${t.windowId + 1}`,
      icon: <IconTerminal className={iconCls} />,
    };
  }
  if (t.kind === "files")
    return { label: t.label ?? "Files", icon: <IconFolder className={iconCls} /> };
  if (t.kind === "gui") return { label: t.label ?? "GUI", icon: glyph("▢") };
  if (t.kind === "diff") {
    const base = t.file.split("/").pop() || t.file;
    return { label: t.label ?? `${base} (diff)`, icon: glyph("Δ") };
  }
  if (t.kind === "graph")
    return { label: t.label ?? "Graph", icon: <IconBranch className={iconCls} /> };
  if (t.kind === "tree")
    return { label: t.label ?? "Tree", icon: <IconFolder className={iconCls} /> };
  if (t.kind === "chat") return { label: t.label ?? "Chat", icon: glyph("◇") };
  const base = t.path.split("/").pop() || t.path;
  return {
    label: (
      <>
        {t.label ?? base}
        {t.dirty && <span className="ml-1 text-warn">•</span>}
      </>
    ),
    icon: glyph("≡"),
  };
}

export function TabBar({
  tabs,
  activeId,
  onSelect,
  onClose,
  onAddTerminal,
  onAddFiles,
  onAddGui,
  onAddGraph,
  onAddTree,
  canAddFiles,
}: {
  tabs: Tab[];
  activeId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onAddTerminal: () => void;
  onAddFiles: () => void;
  onAddGui: () => void;
  onAddGraph: () => void;
  onAddTree: () => void;
  canAddFiles: boolean;
}) {
  const items: TabItem[] = tabs.map((t) => {
    const { label, icon } = describe(t);
    return { id: t.id, label, icon, closable: true, closeLabel: "タブを閉じる" };
  });

  const cwdHint = (ok: boolean) =>
    !ok && <span className="ml-1 text-fg-faint text-2xs">(cwd 不明)</span>;

  return (
    <div className="flex items-stretch min-w-0 border-b border-line bg-surface-muted">
      <Tabs
        items={items}
        activeId={activeId}
        onActivate={onSelect}
        onClose={onClose}
        aria-label="セッションタブ"
        className="flex-1 min-w-0"
      />
      {/* "+" sticks to the right edge, outside the scrolling tab region. */}
      <div className="shrink-0 border-l border-line flex items-stretch">
        <Menu align="end">
          <MenuButton
            aria-label="タブを追加"
            title="タブを追加"
            className="h-full px-4 sm:px-3 inline-flex items-center text-fg-faint hover:text-fg hover:bg-surface transition-colors"
          >
            <IconPlus className="w-3.5 h-3.5" />
          </MenuButton>
          <MenuList className="w-56">
            <MenuItem leftIcon={<IconTerminal />} onSelect={onAddTerminal}>
              新規ターミナル
            </MenuItem>
            <MenuItem leftIcon={<IconFolder />} disabled={!canAddFiles} onSelect={onAddFiles}>
              ファイル変更{cwdHint(canAddFiles)}
            </MenuItem>
            <MenuItem leftIcon={glyph("▢")} onSelect={onAddGui}>
              GUI (noVNC)
            </MenuItem>
            <MenuItem leftIcon={<IconBranch />} disabled={!canAddFiles} onSelect={onAddGraph}>
              コミットグラフ{cwdHint(canAddFiles)}
            </MenuItem>
            <MenuItem leftIcon={<IconFolder />} disabled={!canAddFiles} onSelect={onAddTree}>
              ファイルツリー{cwdHint(canAddFiles)}
            </MenuItem>
          </MenuList>
        </Menu>
      </div>
    </div>
  );
}

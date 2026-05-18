import { useEffect, useState } from "react";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { Button } from "@/components/ui";

export type DeleteTarget = {
  name: string;
  worktreePath: string;
  origPath: string;
};

type Props = {
  target: DeleteTarget | null;
  onClose: () => void;
  onConfirm: (deleteWorktree: boolean) => Promise<void>;
  busy: boolean;
};

export function DeleteConfirmSheet({ target, onClose, onConfirm, busy }: Props) {
  const [deleteWt, setDeleteWt] = useState(true);

  useEffect(() => {
    if (target) setDeleteWt(true);
  }, [target]);

  const open = !!target;
  const hasWt = !!target?.worktreePath;

  return (
    <Sheet open={open} onClose={onClose} maxWidth="md">
      <SheetHeader title="セッションを停止" onClose={onClose} />
      <SheetBody>
        <div className="text-sm text-fg-muted">
          <span className="font-mono text-xs break-all bg-surface-muted px-2 py-1 rounded border border-line">
            {target?.name}
          </span>
          <span className="ml-2">を停止する</span>
        </div>
        {hasWt && (
          <div className="flex items-start justify-between gap-3 p-3.5 border border-amber-500/30 bg-amber-500/5 rounded-lg">
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-fg">worktree も削除する</div>
              <div className="text-xs text-fg-subtle break-all mt-1 font-mono">
                {target?.worktreePath}
              </div>
              <div className="text-xs text-fg-faint mt-1">
                <code className="font-mono">agent-start/*</code> ブランチも削除
              </div>
            </div>
            <Toggle checked={deleteWt} onChange={setDeleteWt} tone="danger" />
          </div>
        )}
      </SheetBody>
      <SheetFooter>
        <Button variant="secondary" size="lg" onClick={onClose} disabled={busy} className="flex-1">
          キャンセル
        </Button>
        <Button
          variant="danger"
          size="lg"
          loading={busy}
          onClick={() => onConfirm(hasWt && deleteWt)}
          className="flex-1"
        >
          停止
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

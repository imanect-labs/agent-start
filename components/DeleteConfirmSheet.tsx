"use client";

import { Button } from "@heroui/react";
import { useEffect, useState } from "react";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";

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

export function DeleteConfirmSheet({
  target,
  onClose,
  onConfirm,
  busy,
}: Props) {
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
        <div className="text-sm text-zinc-700">
          <span className="font-mono text-xs break-all bg-zinc-50 px-2 py-1 rounded border border-zinc-200">
            {target?.name}
          </span>
          <span className="ml-2">を停止する。</span>
        </div>
        {hasWt && (
          <div className="flex items-start justify-between gap-3 p-3 bg-amber-50 border border-amber-200 rounded-lg">
            <div className="flex-1">
              <div className="font-semibold text-sm text-zinc-900">
                worktree も削除
              </div>
              <div className="text-xs text-zinc-500 break-all mt-0.5 font-mono">
                {target?.worktreePath}
              </div>
              <div className="text-xs text-zinc-500 mt-0.5">
                ccstart/* ブランチも削除
              </div>
            </div>
            <Toggle checked={deleteWt} onChange={setDeleteWt} color="red" />
          </div>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="bordered"
          onPress={onClose}
          isDisabled={busy}
          className="flex-1 min-h-12 border-zinc-300 text-zinc-700"
          disableRipple
        >
          キャンセル
        </Button>
        <Button
          color="danger"
          isLoading={busy}
          onPress={() => onConfirm(hasWt && deleteWt)}
          className="flex-1 min-h-12 font-bold"
          disableRipple
        >
          停止
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

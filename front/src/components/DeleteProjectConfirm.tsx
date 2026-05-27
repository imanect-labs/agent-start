import { useState } from "react";
import { mutate } from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Button } from "@/components/ui";
import { useToast } from "./Toast";

type Props = {
  open: boolean;
  name: string;
  onClose: () => void;
};

export function DeleteProjectConfirm({ open, name, onClose }: Props) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    // Optimistically drop the project so the sidebar updates instantly.
    mutate(
      "/api/projects",
      (cur?: { projects: { name: string }[]; pending?: unknown[] }) =>
        cur ? { ...cur, projects: (cur.projects ?? []).filter((p) => p.name !== name) } : cur,
      { revalidate: false },
    );
    try {
      const res = await fetch(`/api/projects/${encodeURIComponent(name)}`, {
        method: "DELETE",
      });
      const json = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
      toast({ title: "削除しました", description: name, color: "success" });
      mutate("/api/projects");
      onClose();
    } catch (e) {
      toast({ title: "削除失敗", description: (e as Error).message, color: "danger" });
      // Restore the optimistically removed project on failure.
      mutate("/api/projects");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet open={open} onClose={onClose} maxWidth="md">
      <SheetHeader title="プロジェクトを削除" onClose={onClose} />
      <SheetBody>
        <p className="text-sm text-fg">
          <span className="font-mono break-all">{name}</span> をディスクから完全に削除します。
        </p>
        <p className="mt-2 text-xs text-danger">この操作は取り消せません。</p>
      </SheetBody>
      <SheetFooter>
        <div className="flex-1" />
        <Button variant="secondary" size="md" onClick={onClose} disabled={busy}>
          キャンセル
        </Button>
        <Button variant="danger" size="md" loading={busy} onClick={submit}>
          削除する
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

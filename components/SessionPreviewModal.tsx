"use client";

import { Button, Spinner } from "@heroui/react";
import { useEffect, useState } from "react";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";

type Props = {
  sessionName: string | null;
  isOpen: boolean;
  onClose: () => void;
};

export function SessionPreviewModal({ sessionName, isOpen, onClose }: Props) {
  const [output, setOutput] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen || !sessionName) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const tick = async () => {
      try {
        setLoading(true);
        const res = await fetch(
          `/api/sessions/${encodeURIComponent(sessionName)}/output`,
          { cache: "no-store" },
        );
        const data = await res.json();
        if (!alive) return;
        if (!res.ok) {
          setErr(data.error || `HTTP ${res.status}`);
        } else {
          setOutput(data.output ?? "");
          setErr(null);
        }
      } catch (e) {
        if (alive) setErr((e as Error).message);
      } finally {
        if (alive) {
          setLoading(false);
          timer = setTimeout(tick, 3000);
        }
      }
    };

    tick();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  }, [isOpen, sessionName]);

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="3xl">
      <SheetHeader
        title={
          <span className="font-mono text-sm break-all">{sessionName}</span>
        }
        subtitle={loading ? "更新中..." : undefined}
        onClose={onClose}
      />
      <SheetBody>
        {err && (
          <div className="text-red-600 text-sm bg-red-50 border border-red-200 rounded p-2">
            エラー: {err}
          </div>
        )}
        <pre className="text-xs font-mono whitespace-pre-wrap break-all bg-zinc-50 text-zinc-800 p-3 rounded-md min-h-[50vh] border border-zinc-200">
          {output || (loading ? "読み込み中..." : "(出力なし)")}
        </pre>
      </SheetBody>
      <SheetFooter>
        <Button
          variant="bordered"
          onPress={onClose}
          className="min-h-12 w-full border-zinc-300 text-zinc-700"
          disableRipple
        >
          閉じる
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

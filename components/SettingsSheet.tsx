"use client";

import { useEffect, useState } from "react";
import useSWR, { mutate } from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { useToast } from "./Toast";
import { Button, Input, Spinner } from "@/components/ui";

type Preferences = {
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
};
type CliInfo = {
  key: string;
  label: string;
  command: string;
  hasSkipFlag: boolean;
  skipFlag: string;
};

const fetcher = (url: string) => fetch(url).then((r) => r.json());

type Props = {
  isOpen: boolean;
  onClose: () => void;
};

export function SettingsSheet({ isOpen, onClose }: Props) {
  const toast = useToast();
  const { data: prefData, isLoading: prefLoading } = useSWR<{
    preferences: Preferences;
  }>(isOpen ? "/api/preferences" : null, fetcher);
  const { data: cfgData } = useSWR<{ clis: CliInfo[] }>(
    isOpen ? "/api/config" : null,
    fetcher,
  );

  const [cli, setCli] = useState("claude");
  const [skip, setSkip] = useState(false);
  const [extra, setExtra] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (prefData?.preferences) {
      setCli(prefData.preferences.cli);
      setSkip(prefData.preferences.skipPermissions);
      setExtra(prefData.preferences.extraArgs);
    }
  }, [prefData]);

  const selectedCli = cfgData?.clis.find((c) => c.key === cli);
  const clis = cfgData?.clis ?? [];

  const save = async () => {
    setSaving(true);
    try {
      const res = await fetch("/api/preferences", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ cli, skipPermissions: skip, extraArgs: extra }),
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      toast({ title: "保存しました", color: "success" });
      mutate("/api/preferences");
      onClose();
    } catch (e) {
      toast({
        title: "保存に失敗",
        description: (e as Error).message,
        color: "danger",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="md">
      <SheetHeader
        title="既定の設定"
        subtitle="新規セッション起動時のデフォルト"
        onClose={onClose}
      />
      <SheetBody>
        {prefLoading ? (
          <div className="flex justify-center py-8">
            <Spinner size="md" />
          </div>
        ) : (
          <>
            <div>
              <div className="text-xs font-medium text-zinc-700 mb-2">
                既定の CLI
              </div>
              <div className="grid grid-cols-2 gap-2">
                {clis.map((c) => {
                  const active = cli === c.key;
                  return (
                    <button
                      key={c.key}
                      type="button"
                      onClick={() => setCli(c.key)}
                      className={[
                        "h-auto min-h-[3.25rem] px-3 py-2 rounded-md border text-left transition-colors",
                        active
                          ? "border-zinc-900 bg-zinc-900 text-white"
                          : "border-zinc-200 bg-white text-zinc-700 hover:border-zinc-300 hover:bg-zinc-50",
                      ].join(" ")}
                    >
                      <div className="text-sm font-medium">{c.label}</div>
                      <div
                        className={[
                          "text-[11px] font-mono truncate mt-0.5",
                          active ? "text-zinc-300" : "text-zinc-500",
                        ].join(" ")}
                      >
                        {c.command || "default-shell"}
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="flex items-center justify-between gap-3">
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium text-zinc-800">
                  権限プロンプトをスキップ
                </div>
                <div className="text-xs text-zinc-500 font-mono break-all mt-0.5">
                  {selectedCli?.hasSkipFlag ? selectedCli.skipFlag : "(未対応)"}
                </div>
              </div>
              <Toggle
                checked={skip}
                onChange={setSkip}
                disabled={!selectedCli?.hasSkipFlag}
              />
            </div>

            <Input
              label="追加フラグ (任意)"
              placeholder="例: --model claude-opus-4-7"
              value={extra}
              onValueChange={setExtra}
              description="CLI コマンドに追記。英数字・空白・- _ . / = のみ"
            />
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="secondary"
          size="lg"
          onClick={onClose}
          className="flex-1"
        >
          キャンセル
        </Button>
        <Button
          variant="primary"
          size="lg"
          loading={saving}
          onClick={save}
          className="flex-1"
        >
          保存
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

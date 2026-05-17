"use client";

import { Button, Input, Spinner } from "@heroui/react";
import { useEffect, useState } from "react";
import useSWR, { mutate } from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { useToast } from "./Toast";

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
          <div className="flex justify-center py-6">
            <Spinner size="sm" />
          </div>
        ) : (
          <>
            <div>
              <div className="text-sm font-semibold text-zinc-700 mb-2">
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
                      className={`min-h-12 px-3 py-2 rounded-lg border text-left ${
                        active
                          ? "border-blue-500 bg-blue-50 text-blue-900"
                          : "border-zinc-200 bg-white text-zinc-700"
                      }`}
                    >
                      <div className="font-semibold text-sm">{c.label}</div>
                      <div className="text-xs text-zinc-500 font-mono truncate">
                        {c.command}
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="flex items-center justify-between gap-3">
              <div className="flex-1">
                <div className="font-semibold text-sm text-zinc-700">
                  権限プロンプトをスキップ
                </div>
                <div className="text-xs text-zinc-400 font-mono break-all">
                  {selectedCli?.hasSkipFlag
                    ? selectedCli.skipFlag
                    : "(未対応)"}
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
              description="CLI コマンドに追記。英数字・空白・- _ . / = のみ。"
              variant="bordered"
              size="sm"
              classNames={{
                inputWrapper:
                  "border-zinc-300 data-[hover=true]:border-zinc-400 data-[focus=true]:border-blue-500",
              }}
            />
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="bordered"
          onPress={onClose}
          className="flex-1 min-h-12 border-zinc-300 text-zinc-700"
          disableRipple
        >
          キャンセル
        </Button>
        <Button
          color="primary"
          onPress={save}
          isLoading={saving}
          className="flex-1 min-h-12 font-bold"
          disableRipple
        >
          保存
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

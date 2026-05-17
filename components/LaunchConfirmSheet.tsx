"use client";

import { Button, Input, Spinner } from "@heroui/react";
import { useEffect, useState } from "react";
import useSWR from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";

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

export type LaunchOverrides = {
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
  createWorktree: boolean;
};

type Props = {
  isOpen: boolean;
  projectName: string;
  projectPath: string;
  isGit: boolean;
  onClose: () => void;
  onLaunch: (overrides: LaunchOverrides) => Promise<void>;
  launching: boolean;
};

export function LaunchConfirmSheet({
  isOpen,
  projectName,
  projectPath,
  isGit,
  onClose,
  onLaunch,
  launching,
}: Props) {
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
  const [createWt, setCreateWt] = useState(true);
  const [showAdv, setShowAdv] = useState(false);

  useEffect(() => {
    if (prefData?.preferences) {
      setCli(prefData.preferences.cli);
      setSkip(prefData.preferences.skipPermissions);
      setExtra(prefData.preferences.extraArgs);
    }
  }, [prefData]);

  // git リポでない場合は強制 OFF
  useEffect(() => {
    if (!isGit) setCreateWt(false);
    else if (isOpen) setCreateWt(true);
  }, [isGit, isOpen]);

  useEffect(() => {
    if (!isOpen) setShowAdv(false);
  }, [isOpen]);

  const selectedCli = cfgData?.clis.find((c) => c.key === cli);
  const clis = cfgData?.clis ?? [];

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="md">
      <SheetHeader
        title={projectName}
        subtitle={projectPath}
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
              <div className="text-sm font-semibold text-zinc-700 mb-2">CLI</div>
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
                    : "(この CLI は未対応)"}
                </div>
              </div>
              <Toggle
                checked={skip}
                onChange={setSkip}
                disabled={!selectedCli?.hasSkipFlag}
              />
            </div>

            <div className="flex items-center justify-between gap-3">
              <div className="flex-1">
                <div className="font-semibold text-sm text-zinc-700">
                  worktree を作って起動
                </div>
                <div className="text-xs text-zinc-400">
                  {isGit
                    ? "本体に影響を与えず別ブランチで作業"
                    : "git リポジトリではないため使用不可"}
                </div>
              </div>
              <Toggle
                checked={createWt}
                onChange={setCreateWt}
                disabled={!isGit}
              />
            </div>

            <button
              type="button"
              onClick={() => setShowAdv((s) => !s)}
              className="text-xs text-zinc-500 underline self-start"
            >
              {showAdv ? "詳細を閉じる" : "詳細オプション"}
            </button>

            {showAdv && (
              <Input
                label="追加フラグ"
                placeholder="(なし)"
                value={extra}
                onValueChange={setExtra}
                size="sm"
                variant="bordered"
                classNames={{
                  inputWrapper:
                    "border-zinc-300 data-[hover=true]:border-zinc-400 data-[focus=true]:border-blue-500",
                }}
              />
            )}
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="bordered"
          onPress={onClose}
          className="flex-1 min-h-12 border-zinc-300 text-zinc-700"
          disableRipple
          isDisabled={launching}
        >
          キャンセル
        </Button>
        <Button
          color="primary"
          isLoading={launching}
          className="flex-1 min-h-12 font-bold"
          disableRipple
          onPress={() =>
            onLaunch({
              cli,
              skipPermissions: skip,
              extraArgs: extra,
              createWorktree: createWt,
            })
          }
        >
          起動する
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

"use client";

import { useEffect, useState } from "react";
import useSWR from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
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
          <div className="flex justify-center py-8">
            <Spinner size="md" />
          </div>
        ) : (
          <>
            <Section title="CLI">
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
            </Section>

            <Row
              title="権限プロンプトをスキップ"
              hint={
                selectedCli?.hasSkipFlag ? selectedCli.skipFlag : "(この CLI は未対応)"
              }
              mono
            >
              <Toggle
                checked={skip}
                onChange={setSkip}
                disabled={!selectedCli?.hasSkipFlag}
              />
            </Row>

            <Row
              title="worktree を作って起動"
              hint={
                isGit
                  ? "本体に影響を与えず別ブランチで作業"
                  : "git リポジトリではないため使用不可"
              }
            >
              <Toggle checked={createWt} onChange={setCreateWt} disabled={!isGit} />
            </Row>

            <button
              type="button"
              onClick={() => setShowAdv((s) => !s)}
              className="text-xs text-zinc-500 hover:text-zinc-800 transition-colors self-start inline-flex items-center gap-1"
            >
              <span
                className={[
                  "inline-block transition-transform",
                  showAdv ? "rotate-90" : "",
                ].join(" ")}
              >
                ›
              </span>
              詳細オプション
            </button>

            {showAdv && (
              <Input
                label="追加フラグ"
                placeholder="(なし)"
                value={extra}
                onValueChange={setExtra}
                description="英数字・空白・- _ . / = のみ"
              />
            )}
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="secondary"
          size="lg"
          onClick={onClose}
          disabled={launching}
          className="flex-1"
        >
          キャンセル
        </Button>
        <Button
          variant="primary"
          size="lg"
          loading={launching}
          className="flex-1"
          onClick={() =>
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

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="text-xs font-medium text-zinc-700 mb-2">{title}</div>
      {children}
    </div>
  );
}

function Row({
  title,
  hint,
  mono,
  children,
}: {
  title: string;
  hint?: string;
  mono?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium text-zinc-800">{title}</div>
        {hint && (
          <div
            className={[
              "text-xs text-zinc-500 mt-0.5 break-all",
              mono ? "font-mono" : "",
            ].join(" ")}
          >
            {hint}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}

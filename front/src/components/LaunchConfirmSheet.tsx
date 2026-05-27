import { useEffect, useState } from "react";
import useSWR from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { Button, Input, Spinner } from "@/components/ui";

type Preferences = {
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
  createWorktree: boolean;
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
  /** When launching from a GitHub issue, the issue this session targets. */
  issueContext?: { number: number; title: string };
  onClose: () => void;
  onLaunch: (overrides: LaunchOverrides) => Promise<void>;
};

export function LaunchConfirmSheet({
  isOpen,
  projectName,
  projectPath,
  isGit,
  issueContext,
  onClose,
  onLaunch,
}: Props) {
  const { data: prefData, isLoading: prefLoading } = useSWR<{
    preferences: Preferences;
  }>(isOpen ? "/api/preferences" : null, fetcher);
  const { data: cfgData } = useSWR<{ clis: CliInfo[] }>(isOpen ? "/api/config" : null, fetcher);

  const [cli, setCli] = useState("claude");
  const [skip, setSkip] = useState(false);
  const [extra, setExtra] = useState("");
  const [createWt, setCreateWt] = useState(true);
  const [showAdv, setShowAdv] = useState(false);

  // Hydrate form fields from preferences each time the dialog opens.
  useEffect(() => {
    if (!isOpen || !prefData?.preferences) return;
    const p = prefData.preferences;
    setCli(p.cli);
    setSkip(p.skipPermissions);
    setExtra(p.extraArgs);
    // Worktree default = preference, but force off when not a git repo.
    setCreateWt(isGit ? (p.createWorktree ?? true) : false);
  }, [isOpen, prefData, isGit]);

  useEffect(() => {
    if (!isGit) setCreateWt(false);
  }, [isGit]);

  useEffect(() => {
    if (!isOpen) setShowAdv(false);
  }, [isOpen]);

  const selectedCli = cfgData?.clis.find((c) => c.key === cli);
  const clis = cfgData?.clis ?? [];

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="lg">
      <SheetHeader title={projectName} subtitle={projectPath} onClose={onClose} />
      <SheetBody>
        {issueContext && (
          <div className="rounded-md border border-accent/30 bg-accent/5 px-3 py-2 text-xs text-fg">
            <span className="font-medium">issue #{issueContext.number}</span> に取り組みます
            <div className="text-fg-subtle truncate mt-0.5">{issueContext.title}</div>
          </div>
        )}
        {prefLoading ? (
          <div className="flex justify-center py-8">
            <Spinner size="md" />
          </div>
        ) : (
          <>
            <Section title="CLI">
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
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
                          ? "border-accent bg-accent text-accent-fg"
                          : "border-line bg-surface text-fg hover:bg-surface-muted",
                      ].join(" ")}
                    >
                      <div className="text-sm font-medium">{c.label}</div>
                      <div
                        className={[
                          "text-[11px] font-mono truncate mt-0.5",
                          active ? "opacity-70" : "text-fg-subtle",
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
              hint={selectedCli?.hasSkipFlag ? selectedCli.skipFlag : "(この CLI は未対応)"}
              mono
            >
              <Toggle checked={skip} onChange={setSkip} disabled={!selectedCli?.hasSkipFlag} />
            </Row>

            <Row
              title="worktree を作って起動"
              hint={
                isGit ? "本体に影響を与えず別ブランチで作業" : "git リポジトリではないため使用不可"
              }
            >
              <Toggle checked={createWt} onChange={setCreateWt} disabled={!isGit} />
            </Row>

            <button
              type="button"
              onClick={() => setShowAdv((s) => !s)}
              className="text-xs text-fg-subtle hover:text-fg transition-colors self-start inline-flex items-center gap-1"
            >
              <span
                className={["inline-block transition-transform", showAdv ? "rotate-90" : ""].join(
                  " ",
                )}
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
        <Button variant="secondary" size="lg" onClick={onClose} className="flex-1">
          キャンセル
        </Button>
        <Button
          variant="primary"
          size="lg"
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

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-xs font-medium text-fg-muted mb-2">{title}</div>
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
        <div className="text-sm font-medium text-fg">{title}</div>
        {hint && (
          <div
            className={["text-xs text-fg-subtle mt-0.5 break-all", mono ? "font-mono" : ""].join(
              " ",
            )}
          >
            {hint}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}

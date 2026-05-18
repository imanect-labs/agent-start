import { useEffect, useState } from "react";
import useSWR, { mutate } from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { useToast } from "./Toast";
import { Button, Input, Spinner } from "@/components/ui";
import { useTheme, type ThemeChoice } from "@/components/ThemeProvider";
import { IconMonitor, IconMoon, IconSun } from "@/components/icons";

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
type ConfigInfo = {
  clis: CliInfo[];
  defaultCli: string;
  sessionPrefix: string;
  roots: string[];
  shell: string;
  showHidden: boolean;
  gitOnly: boolean;
  paths: {
    config: string;
    preferences: string;
    worktreeRoot: string;
  };
};

const fetcher = (url: string) => fetch(url).then((r) => r.json());

type Props = {
  isOpen: boolean;
  onClose: () => void;
};

export function SettingsDialog({ isOpen, onClose }: Props) {
  const toast = useToast();
  const { theme, setTheme } = useTheme();

  const { data: prefData, isLoading: prefLoading } = useSWR<{
    preferences: Preferences;
  }>(isOpen ? "/api/preferences" : null, fetcher);
  const { data: cfgData, isLoading: cfgLoading } = useSWR<ConfigInfo>(
    isOpen ? "/api/config" : null,
    fetcher,
  );

  const [cli, setCli] = useState("claude");
  const [skip, setSkip] = useState(false);
  const [extra, setExtra] = useState("");
  const [createWt, setCreateWt] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (prefData?.preferences) {
      setCli(prefData.preferences.cli);
      setSkip(prefData.preferences.skipPermissions);
      setExtra(prefData.preferences.extraArgs);
      setCreateWt(prefData.preferences.createWorktree ?? true);
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
        body: JSON.stringify({
          cli,
          skipPermissions: skip,
          extraArgs: extra,
          createWorktree: createWt,
        }),
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

  const loading = prefLoading || cfgLoading;

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="2xl">
      <SheetHeader
        title="設定"
        subtitle="新規セッション起動時のデフォルトと外観設定"
        onClose={onClose}
      />
      <SheetBody>
        {loading ? (
          <div className="flex justify-center py-10">
            <Spinner size="md" />
          </div>
        ) : (
          <>
            <Section title="外観" hint="テーマと表示">
              <ThemeSelector value={theme} onChange={setTheme} />
            </Section>

            <Divider />

            <Section title="起動デフォルト" hint="新規セッション起動時の初期値">
              <div className="space-y-4">
                <Field label="既定の CLI">
                  <div className="grid grid-cols-2 lg:grid-cols-3 gap-2">
                    {clis.map((c) => {
                      const active = cli === c.key;
                      return (
                        <button
                          key={c.key}
                          type="button"
                          onClick={() => setCli(c.key)}
                          className={[
                            "h-auto min-h-[3.5rem] px-3 py-2 rounded-md border text-left transition-colors",
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
                </Field>

                <Row
                  title="権限プロンプトをスキップ"
                  hint={selectedCli?.hasSkipFlag ? selectedCli.skipFlag : "(選択中の CLI は未対応)"}
                  mono
                >
                  <Toggle checked={skip} onChange={setSkip} disabled={!selectedCli?.hasSkipFlag} />
                </Row>

                <Row
                  title="git リポジトリでは worktree を作って起動"
                  hint="本体ブランチに影響を与えず別ブランチで作業"
                >
                  <Toggle checked={createWt} onChange={setCreateWt} />
                </Row>

                <Input
                  label="追加フラグ (任意)"
                  placeholder="例: --model claude-opus-4-7"
                  value={extra}
                  onValueChange={setExtra}
                  description="CLI コマンドに追記。英数字・空白・- _ . / = のみ"
                />
              </div>
            </Section>

            {cfgData && (
              <>
                <Divider />
                <Section title="システム情報" hint="config.json で編集 (要再起動)">
                  <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-xs">
                    <KV k="検索 roots">
                      <ul className="space-y-1">
                        {cfgData.roots.map((r) => (
                          <li key={r} className="font-mono">
                            {r}
                          </li>
                        ))}
                      </ul>
                    </KV>
                    <KV k="セッション接頭辞">
                      <code className="font-mono">{cfgData.sessionPrefix}</code>
                    </KV>
                    <KV k="シェル">
                      <code className="font-mono">{cfgData.shell}</code>
                    </KV>
                    <KV k="git のみ">
                      <code className="font-mono">{cfgData.gitOnly ? "true" : "false"}</code>
                    </KV>
                    <KV k="隠しディレクトリ">
                      <code className="font-mono">{cfgData.showHidden ? "表示" : "非表示"}</code>
                    </KV>
                    <KV k="worktree 置き場">
                      <code className="font-mono break-all">{cfgData.paths.worktreeRoot}</code>
                    </KV>
                    <KV k="設定ファイル">
                      <code className="font-mono break-all">{cfgData.paths.config}</code>
                    </KV>
                    <KV k="preferences">
                      <code className="font-mono break-all">{cfgData.paths.preferences}</code>
                    </KV>
                  </dl>
                </Section>
              </>
            )}
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <div className="flex-1" />
        <Button variant="secondary" size="md" onClick={onClose}>
          キャンセル
        </Button>
        <Button variant="primary" size="md" loading={saving} onClick={save}>
          保存
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

function ThemeSelector({
  value,
  onChange,
}: {
  value: ThemeChoice;
  onChange: (v: ThemeChoice) => void;
}) {
  const items: { key: ThemeChoice; label: string; icon: React.ReactNode }[] = [
    { key: "light", label: "ライト", icon: <IconSun className="w-4 h-4" /> },
    { key: "dark", label: "ダーク", icon: <IconMoon className="w-4 h-4" /> },
    {
      key: "system",
      label: "システム",
      icon: <IconMonitor className="w-4 h-4" />,
    },
  ];
  return (
    <div className="grid grid-cols-3 gap-2">
      {items.map((it) => {
        const active = value === it.key;
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onChange(it.key)}
            className={[
              "h-12 inline-flex items-center justify-center gap-2 rounded-md border transition-colors text-sm font-medium",
              active
                ? "border-accent bg-accent text-accent-fg"
                : "border-line bg-surface text-fg hover:bg-surface-muted",
            ].join(" ")}
          >
            {it.icon}
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <div className="mb-3">
        <div className="text-[11px] uppercase tracking-wider text-fg-subtle font-medium">
          {title}
        </div>
        {hint && <div className="text-[11px] text-fg-faint mt-0.5">{hint}</div>}
      </div>
      {children}
    </section>
  );
}

function Divider() {
  return <div className="h-px bg-line my-1" />;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-xs font-medium text-fg-muted mb-2">{label}</div>
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

function KV({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-fg-subtle whitespace-nowrap">{k}</dt>
      <dd className="text-fg break-all min-w-0">{children}</dd>
    </>
  );
}

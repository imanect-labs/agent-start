import { useEffect, useMemo, useState } from "react";
import useSWR, { mutate } from "swr";
import { useBlocker, useNavigate } from "@tanstack/react-router";
import { Toggle } from "./Toggle";
import { useToast } from "./Toast";
import { Button, Card, Input, SegmentedControl, Spinner } from "@/components/ui";
import { useTheme, type ThemeChoice } from "@/components/ThemeProvider";
import { IconMonitor, IconMoon, IconSun } from "@/components/icons";
import { readSendKey, SEND_KEY_STORAGE, type SendKey } from "@/components/chat/ChatComposer";

type Preferences = {
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
  createWorktree: boolean;
  guiOpenInNewTab: boolean;
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

type FormState = {
  // preferences
  cli: string;
  skipPermissions: boolean;
  extraArgs: string;
  createWorktree: boolean;
  guiOpenInNewTab: boolean;
  // config
  roots: string;
  sessionPrefix: string;
  shell: string;
  showHidden: boolean;
  gitOnly: boolean;
  defaultCli: string;
};

const empty: FormState = {
  cli: "claude",
  skipPermissions: false,
  extraArgs: "",
  createWorktree: true,
  guiOpenInNewTab: false,
  roots: "",
  sessionPrefix: "cc-",
  shell: "/bin/bash",
  showHidden: false,
  gitOnly: false,
  defaultCli: "claude",
};

export function SettingsPage() {
  const toast = useToast();
  const navigate = useNavigate();
  const { theme, setTheme } = useTheme();

  const { data: prefData, isLoading: prefLoading } = useSWR<{ preferences: Preferences }>(
    "/api/preferences",
    fetcher,
  );
  const { data: cfgData, isLoading: cfgLoading } = useSWR<ConfigInfo>("/api/config", fetcher);

  const [form, setForm] = useState<FormState>(empty);
  // The last-saved snapshot. Kept in state (not a ref) so that updating it
  // after a successful save re-renders and recomputes `dirty` — a ref mutation
  // wouldn't, leaving the UI stuck on "未保存".
  const [saved, setSaved] = useState<FormState>(empty);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!prefData?.preferences || !cfgData) return;
    const next: FormState = {
      cli: prefData.preferences.cli,
      skipPermissions: prefData.preferences.skipPermissions,
      extraArgs: prefData.preferences.extraArgs,
      createWorktree: prefData.preferences.createWorktree ?? true,
      guiOpenInNewTab: prefData.preferences.guiOpenInNewTab ?? false,
      roots: cfgData.roots.join("\n"),
      sessionPrefix: cfgData.sessionPrefix,
      shell: cfgData.shell,
      showHidden: cfgData.showHidden,
      gitOnly: cfgData.gitOnly,
      defaultCli: cfgData.defaultCli,
    };
    setForm(next);
    setSaved(next);
  }, [prefData, cfgData]);

  const dirty = useMemo(() => {
    return JSON.stringify(form) !== JSON.stringify(saved);
  }, [form, saved]);

  // Block in-app navigation when dirty
  useBlocker({
    shouldBlockFn: () => {
      if (!dirty) return false;
      return !window.confirm("未保存の変更があります。破棄して移動しますか?");
    },
  });

  // Warn on browser-level close/reload
  useEffect(() => {
    if (!dirty) return;
    const handler = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [dirty]);

  const selectedCli = cfgData?.clis.find((c) => c.key === form.cli);
  const clis = cfgData?.clis ?? [];

  const update = <K extends keyof FormState>(k: K, v: FormState[K]) =>
    setForm((p) => ({ ...p, [k]: v }));

  const save = async () => {
    setSaving(true);
    try {
      const rootsArr = form.roots
        .split(/\r?\n/)
        .map((s) => s.trim())
        .filter(Boolean);
      if (rootsArr.length === 0) {
        toast({ title: "プロジェクトディレクトリは1つ以上必要です", color: "danger" });
        setSaving(false);
        return;
      }
      const cfgRes = await fetch("/api/config", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          roots: rootsArr,
          sessionPrefix: form.sessionPrefix,
          shell: form.shell,
          showHidden: form.showHidden,
          gitOnly: form.gitOnly,
          defaultCli: form.defaultCli,
        }),
      });
      const cfgJson = await cfgRes.json();
      if (!cfgRes.ok) throw new Error(cfgJson.error ?? `config HTTP ${cfgRes.status}`);

      const prefRes = await fetch("/api/preferences", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          cli: form.cli,
          skipPermissions: form.skipPermissions,
          extraArgs: form.extraArgs,
          createWorktree: form.createWorktree,
          guiOpenInNewTab: form.guiOpenInNewTab,
        }),
      });
      const prefJson = await prefRes.json();
      if (!prefRes.ok) throw new Error(prefJson.error ?? `prefs HTTP ${prefRes.status}`);

      toast({ title: "保存しました", color: "success" });
      mutate("/api/preferences");
      mutate("/api/config");
      setSaved(form);
    } catch (e) {
      toast({ title: "保存失敗", description: (e as Error).message, color: "danger" });
    } finally {
      setSaving(false);
    }
  };

  const cancel = () => {
    if (dirty && !window.confirm("変更を破棄して戻りますか?")) return;
    navigate({ to: "/" });
  };

  const loading = prefLoading || cfgLoading;

  return (
    <div className="h-[var(--app-h)] bg-app text-fg flex flex-col">
      <header className="sticky top-0 z-10 bg-surface border-b border-line safe-top">
        <div className="max-w-5xl mx-auto w-full px-3 sm:px-4 py-2.5 sm:py-3 flex items-center gap-2 sm:gap-3">
          <button
            type="button"
            onClick={cancel}
            className="h-10 sm:h-auto -ml-2 sm:ml-0 px-2 inline-flex items-center text-sm text-fg-subtle hover:text-fg"
            aria-label="戻る"
          >
            ← 戻る
          </button>
          <h1 className="text-base font-semibold tracking-tight flex-1 truncate">設定</h1>
          {dirty && <span className="hidden sm:inline text-2xs text-warn">未保存</span>}
          {/* キャンセルは「戻る」と同じ動作なのでモバイルでは省略。
              dirty 時の警告ドットは「保存」ボタン左に小さく出す。 */}
          <Button
            variant="secondary"
            size="sm"
            onClick={cancel}
            disabled={saving}
            className="hidden sm:inline-flex"
          >
            キャンセル
          </Button>
          {dirty && (
            <span aria-hidden className="sm:hidden inline-block w-1.5 h-1.5 rounded-full bg-warn" />
          )}
          <Button variant="primary" size="sm" loading={saving} onClick={save} disabled={!dirty}>
            保存
          </Button>
        </div>
      </header>

      <main className="flex-1 min-h-0 overflow-y-auto">
        <div className="max-w-5xl mx-auto w-full px-4 py-6 space-y-8">
          {loading ? (
            <div className="flex justify-center py-12">
              <Spinner size="md" />
            </div>
          ) : (
            <>
              <Section title="外観" hint="テーマと表示">
                <ThemeSelector value={theme} onChange={setTheme} />
              </Section>

              <Section
                title="チャット"
                hint="この端末のみに適用 (即時保存・サーバーには送信されません)"
              >
                <Field label="メッセージの送信キー">
                  <SendKeySelector />
                </Field>
              </Section>

              <Section
                title="プロジェクトディレクトリ"
                hint="プロジェクトを探す検索先(1 行 1 パス)。デフォルトは ~/.agent-start/projects"
              >
                <textarea
                  value={form.roots}
                  onChange={(e) => update("roots", e.target.value)}
                  rows={3}
                  className="w-full min-h-[88px] resize-y rounded border border-line bg-app px-3 py-2 text-sm font-mono outline-none transition-colors focus-visible:border-accent/60 focus-visible:ring-2 focus-visible:ring-ring/20"
                />
              </Section>

              <Section title="起動デフォルト" hint="新規セッション起動時の初期値">
                <div className="space-y-4">
                  <Field label="既定の CLI">
                    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
                      {clis.map((c) => {
                        const active = form.cli === c.key;
                        return (
                          <button
                            key={c.key}
                            type="button"
                            onClick={() => update("cli", c.key)}
                            className={[
                              "h-auto min-h-[3.5rem] px-3 py-2 rounded border text-left transition-colors",
                              active
                                ? "border-accent bg-accent text-accent-fg"
                                : "border-line bg-surface text-fg hover:bg-surface-muted",
                            ].join(" ")}
                          >
                            <div className="text-sm font-medium">{c.label}</div>
                            <div
                              className={[
                                "text-2xs font-mono truncate mt-0.5",
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
                    hint={
                      selectedCli?.hasSkipFlag ? selectedCli.skipFlag : "(選択中の CLI は未対応)"
                    }
                    mono
                  >
                    <Toggle
                      checked={form.skipPermissions}
                      onChange={(v) => update("skipPermissions", v)}
                      disabled={!selectedCli?.hasSkipFlag}
                    />
                  </Row>

                  <Row
                    title="git リポジトリでは worktree を作って起動"
                    hint="本体ブランチに影響を与えず別ブランチで作業"
                  >
                    <Toggle
                      checked={form.createWorktree}
                      onChange={(v) => update("createWorktree", v)}
                    />
                  </Row>

                  <Row
                    title="GUI を新しいタブで全画面表示"
                    hint="OFF のときはアプリ内 iframe で表示"
                  >
                    <Toggle
                      checked={form.guiOpenInNewTab}
                      onChange={(v) => update("guiOpenInNewTab", v)}
                    />
                  </Row>

                  <Input
                    label="追加フラグ (任意)"
                    placeholder="例: --model claude-opus-4-7"
                    value={form.extraArgs}
                    onValueChange={(v) => update("extraArgs", v)}
                    description="CLI コマンドに追記"
                  />
                </div>
              </Section>

              <Section title="セッション">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <Input
                    label="セッション接頭辞"
                    value={form.sessionPrefix}
                    onValueChange={(v) => update("sessionPrefix", v)}
                  />
                  <Input
                    label="シェル"
                    value={form.shell}
                    onValueChange={(v) => update("shell", v)}
                  />
                  <Field label="既定の CLI キー">
                    <select
                      value={form.defaultCli}
                      onChange={(e) => update("defaultCli", e.target.value)}
                      className="w-full rounded border border-line bg-app px-3 py-2 text-sm"
                    >
                      {clis.map((c) => (
                        <option key={c.key} value={c.key}>
                          {c.label} ({c.key})
                        </option>
                      ))}
                    </select>
                  </Field>
                </div>
                <div className="mt-4 space-y-3">
                  <Row title="隠しディレクトリも表示" hint="ドットで始まる項目">
                    <Toggle checked={form.showHidden} onChange={(v) => update("showHidden", v)} />
                  </Row>
                  <Row title="git リポジトリのみ表示">
                    <Toggle checked={form.gitOnly} onChange={(v) => update("gitOnly", v)} />
                  </Row>
                </div>
              </Section>

              {cfgData && (
                <Section title="パス" hint="読み取り専用">
                  <dl className="grid grid-cols-1 md:grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-xs">
                    <KV k="設定ファイル">
                      <code className="font-mono break-all">{cfgData.paths.config}</code>
                    </KV>
                    <KV k="preferences">
                      <code className="font-mono break-all">{cfgData.paths.preferences}</code>
                    </KV>
                    <KV k="worktree 置き場">
                      <code className="font-mono break-all">{cfgData.paths.worktreeRoot}</code>
                    </KV>
                  </dl>
                </Section>
              )}
            </>
          )}
        </div>
      </main>
    </div>
  );
}

function SendKeySelector() {
  const [value, setValue] = useState<SendKey>(readSendKey);
  const items: { key: SendKey; label: string; hint: string }[] = [
    { key: "enter", label: "Enter で送信", hint: "Shift+Enter で改行" },
    { key: "ctrlEnter", label: "Ctrl+Enter で送信", hint: "Enter で改行" },
  ];
  const choose = (k: SendKey) => {
    setValue(k);
    try {
      window.localStorage.setItem(SEND_KEY_STORAGE, k);
      // Notify any open chat composer in this same tab to pick up the change.
      window.dispatchEvent(new StorageEvent("storage", { key: SEND_KEY_STORAGE, newValue: k }));
    } catch {
      // ignore
    }
  };
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-w-md">
      {items.map((it) => {
        const active = value === it.key;
        return (
          <button
            key={it.key}
            type="button"
            aria-pressed={active}
            onClick={() => choose(it.key)}
            className={[
              "h-auto min-h-[3.25rem] px-3 py-2 rounded border text-left transition-colors",
              active
                ? "border-accent bg-accent text-accent-fg"
                : "border-line bg-surface text-fg hover:bg-surface-muted",
            ].join(" ")}
          >
            <div className="text-sm font-medium">{it.label}</div>
            <div
              className={["text-2xs mt-0.5", active ? "opacity-70" : "text-fg-subtle"].join(" ")}
            >
              {it.hint}
            </div>
          </button>
        );
      })}
    </div>
  );
}

function ThemeSelector({
  value,
  onChange,
}: {
  value: ThemeChoice;
  onChange: (v: ThemeChoice) => void;
}) {
  return (
    <SegmentedControl<ThemeChoice>
      aria-label="テーマ"
      value={value}
      onChange={onChange}
      options={[
        { value: "light", label: "ライト", icon: <IconSun className="w-4 h-4" /> },
        { value: "dark", label: "ダーク", icon: <IconMoon className="w-4 h-4" /> },
        { value: "system", label: "システム", icon: <IconMonitor className="w-4 h-4" /> },
      ]}
    />
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
      <div className="mb-3 px-1">
        <div className="text-2xs uppercase tracking-wider text-fg-subtle font-medium">{title}</div>
        {hint && <div className="text-2xs text-fg-faint mt-0.5">{hint}</div>}
      </div>
      <Card>
        <div className="p-4">{children}</div>
      </Card>
    </section>
  );
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

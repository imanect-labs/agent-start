import { useEffect, useState } from "react";
import useSWR from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Toggle } from "./Toggle";
import { Button, Spinner } from "@/components/ui";
import { useToast } from "./Toast";

type Project = { name: string; path: string; isGit: boolean };
type CliInfo = { key: string; label: string; command: string; mode?: string };

async function errorMessage(res: Response): Promise<string> {
  try {
    const body = await res.json();
    return body?.error ?? `${res.status} ${res.statusText}`;
  } catch {
    return `${res.status} ${res.statusText}`;
  }
}

const fetcher = async (url: string) => {
  const res = await fetch(url);
  // Without this a 500 is treated as data, and the sheet renders empty
  // menus as though the user simply had no projects.
  if (!res.ok) throw new Error(await errorMessage(res));
  return res.json();
};

/**
 * "このリポジトリにこれをやって" — the phone-sized entry point to the
 * task queue.
 *
 * Deliberately short: a project, a sentence, and go. Everything else
 * (which node, how much CPU, how many retries) has a default, because
 * the whole point is that this can be filled in one-handed while the
 * cluster figures out the rest.
 */
export function NewTaskSheet({
  open,
  onClose,
  onSubmitted,
  /** Preselect a project when opened from its launcher. */
  projectPath,
}: {
  open: boolean;
  onClose: () => void;
  onSubmitted: (taskId: string) => void;
  projectPath?: string;
}) {
  const toast = useToast();
  const {
    data: projData,
    error: projError,
    mutate: reloadProjects,
  } = useSWR<{ projects: Project[] }>(open ? "/api/projects" : null, fetcher);
  const {
    data: cfgData,
    error: cfgError,
    mutate: reloadConfig,
  } = useSWR<{ clis: CliInfo[]; defaultCli: string }>(open ? "/api/config" : null, fetcher);
  const loadError = projError ?? cfgError;

  const [project, setProject] = useState(projectPath ?? "");
  const [prompt, setPrompt] = useState("");
  const [agent, setAgent] = useState("");
  const [createPr, setCreatePr] = useState(true);
  const [draftPr, setDraftPr] = useState(true);
  const [submitting, setSubmitting] = useState(false);

  // Only agents that can run unattended: a chat conversation waits for a
  // person, and the bare shell has nothing to hand a prompt to. The
  // `trim` matches the server's own check, so the menu cannot offer
  // something the API will reject.
  const agents = (cfgData?.clis ?? []).filter((c) => c.mode !== "chat" && c.command.trim() !== "");
  // Tasks branch and push, so a project without git cannot host one.
  const projects = (projData?.projects ?? []).filter((p) => p.isGit);

  useEffect(() => {
    if (!open) return;
    setProject(projectPath ?? "");
    setPrompt("");
    // The configured default may well be a chat agent or the bare
    // shell — neither of which can run a task. Preselecting it would
    // send the user straight into a 400 from a menu that never offered
    // it, so fall back to something that can actually run.
    const preferred = cfgData?.defaultCli ?? "";
    const usable = agents.some((c) => c.key === preferred);
    setAgent(usable ? preferred : (agents[0]?.key ?? ""));
    // `agents` is derived from cfgData, which is already a dependency.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, projectPath, cfgData]);

  const canSubmit = project !== "" && prompt.trim() !== "" && agent !== "" && !submitting;

  async function submit() {
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      const res = await fetch("/api/tasks", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          projectPath: project,
          prompt: prompt.trim(),
          agent: agent || undefined,
          createPr,
          draftPr,
        }),
      });
      if (!res.ok) throw new Error(await errorMessage(res));
      const body = await res.json();
      toast({ title: "タスクを投入しました", color: "success" });
      onSubmitted(body?.task?.id ?? "");
    } catch (e) {
      toast({
        title: "タスクを投入できませんでした",
        description: e instanceof Error ? e.message : String(e),
        color: "danger",
      });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Sheet open={open} onClose={onClose} maxWidth="lg">
      <SheetHeader
        title="タスクを投げる"
        subtitle="空いているノードで実行し、完了したら PR にします"
        onClose={onClose}
      />
      <SheetBody>
        {loadError ? (
          <div className="rounded-lg border border-danger/40 bg-danger-soft px-3 py-2 text-[13px] text-danger">
            <div>読み込みに失敗しました: {String(loadError.message ?? loadError)}</div>
            <button
              type="button"
              onClick={() => {
                void reloadProjects();
                void reloadConfig();
              }}
              className="mt-1.5 text-[12px] underline underline-offset-2"
            >
              再試行
            </button>
          </div>
        ) : !projData || !cfgData ? (
          <div className="flex justify-center py-8">
            <Spinner size="md" />
          </div>
        ) : (
          <>
            <div>
              <label
                htmlFor="task-project"
                className="text-xs font-medium text-fg-muted mb-1.5 block"
              >
                プロジェクト
              </label>
              <select
                id="task-project"
                value={project}
                onChange={(e) => setProject(e.target.value)}
                className="w-full h-10 px-2.5 rounded-md bg-surface border border-line text-sm text-fg"
              >
                <option value="">選択してください</option>
                {projects.map((p) => (
                  <option key={p.path} value={p.path}>
                    {p.name}
                  </option>
                ))}
              </select>
              {projects.length === 0 && (
                <div className="mt-1 text-[11.5px] text-fg-subtle">
                  git リポジトリのプロジェクトがありません。
                </div>
              )}
            </div>

            <div>
              <label
                htmlFor="task-prompt"
                className="text-xs font-medium text-fg-muted mb-1.5 block"
              >
                やってほしいこと
              </label>
              <textarea
                id="task-prompt"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                rows={5}
                placeholder="例: ログイン画面のバリデーションを直して、テストも足して"
                // 16px on touch keeps iOS Safari from zooming the page.
                className="w-full resize-y rounded-md bg-surface border border-line px-2.5 py-2 text-[14px] [@media(pointer:coarse)]:text-[16px] text-fg placeholder:text-fg-faint outline-none focus:border-line-strong"
              />
            </div>

            <div>
              <div className="text-xs font-medium text-fg-muted mb-2">エージェント</div>
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                {agents.map((c) => {
                  const active = agent === c.key;
                  return (
                    <button
                      key={c.key}
                      type="button"
                      onClick={() => setAgent(c.key)}
                      className={[
                        "h-auto min-h-[2.75rem] px-3 py-2 rounded-md border text-left transition-colors",
                        active
                          ? "border-accent bg-accent text-accent-fg"
                          : "border-line bg-surface text-fg hover:bg-surface-muted",
                      ].join(" ")}
                    >
                      <div className="text-sm font-medium truncate">{c.label}</div>
                    </button>
                  );
                })}
              </div>
              {agents.length === 0 && (
                <div className="mt-1 text-[11.5px] text-fg-subtle">
                  タスクを実行できるエージェントが設定にありません。
                </div>
              )}
            </div>

            <Row title="完了したら PR を作る" hint="ブランチは常に push されます">
              <Toggle checked={createPr} onChange={setCreatePr} />
            </Row>
            <Row title="ドラフト PR にする" hint="レビュー前提の下書きとして開きます">
              <Toggle checked={draftPr} onChange={setDraftPr} disabled={!createPr} />
            </Row>
          </>
        )}
      </SheetBody>
      <SheetFooter>
        <Button
          variant="secondary"
          size="lg"
          onClick={onClose}
          disabled={submitting}
          className="flex-1"
        >
          キャンセル
        </Button>
        <Button
          variant="primary"
          size="lg"
          loading={submitting}
          disabled={!canSubmit}
          className="flex-1"
          onClick={submit}
        >
          投げる
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

function Row({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium text-fg">{title}</div>
        {hint && <div className="text-xs text-fg-subtle mt-0.5">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

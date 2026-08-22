import { useState } from "react";
import useSWR, { mutate } from "swr";
import { Link } from "@tanstack/react-router";
import { Badge, Button, Spinner } from "@/components/ui";
import { useToast } from "./Toast";
import { IconChevronRight, IconRefresh } from "@/components/icons";
import { NewTaskSheet } from "./NewTaskSheet";

export type TaskSummary = {
  id: string;
  title: string;
  prompt: string;
  projectPath: string;
  agent: string;
  status: "pending" | "assigned" | "running" | "succeeded" | "failed" | "cancelled";
  attempts: number;
  maxAttempts: number;
  baseBranch: string;
  nodeId: string;
  nodeName: string;
  sessionName: string;
  prUrl: string;
  branch: string;
  notes: string[];
  error: string;
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
};

type TasksBody = { tasks: TaskSummary[] };

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
  // Without this an error page would be treated as data, and a broken
  // backend would render as "you have no tasks".
  if (!res.ok) throw new Error(await errorMessage(res));
  return res.json();
};

const STATUS: Record<
  TaskSummary["status"],
  { tone: "emerald" | "amber" | "red" | "neutral"; label: string }
> = {
  pending: { tone: "neutral", label: "待機中" },
  assigned: { tone: "amber", label: "割り当て中" },
  running: { tone: "amber", label: "実行中" },
  succeeded: { tone: "emerald", label: "完了" },
  failed: { tone: "red", label: "失敗" },
  cancelled: { tone: "neutral", label: "キャンセル" },
};

const ACTIVE: TaskSummary["status"][] = ["pending", "assigned", "running"];

function when(ms: number | undefined): string {
  if (!ms) return "";
  const d = new Date(ms);
  return d.toLocaleString(undefined, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function projectName(path: string): string {
  return path.split("/").filter(Boolean).slice(-1)[0] ?? path;
}

export function TasksPage() {
  const toast = useToast();
  const { data, isLoading, error } = useSWR<TasksBody>("/api/tasks", fetcher, {
    // Tasks move on their own (a node picks one up, an agent finishes),
    // so the list has to poll rather than wait for a user action.
    refreshInterval: 4000,
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);

  async function act(task: TaskSummary, action: "cancel" | "retry") {
    setBusy(task.id);
    try {
      const res = await fetch(`/api/tasks/${task.id}/${action}`, { method: "POST" });
      if (!res.ok) throw new Error(await errorMessage(res));
      await mutate("/api/tasks");
      toast({
        title: action === "cancel" ? "タスクを停止しました" : "タスクを再実行します",
        color: "success",
      });
    } catch (e) {
      toast({
        title: action === "cancel" ? "停止できませんでした" : "再実行できませんでした",
        description: e instanceof Error ? e.message : String(e),
        color: "danger",
      });
    } finally {
      setBusy(null);
    }
  }

  const tasks = data?.tasks ?? [];
  const active = tasks.filter((t) => ACTIVE.includes(t.status));
  const done = tasks.filter((t) => !ACTIVE.includes(t.status));

  return (
    <div className="min-h-dvh bg-surface text-fg">
      <header className="px-4 py-3 border-b border-line flex items-center gap-2">
        <Link
          to="/"
          className="text-[12px] text-fg-subtle hover:text-fg inline-flex items-center gap-1"
        >
          agent-start
          <IconChevronRight className="w-3 h-3" />
        </Link>
        <h1 className="text-sm font-semibold tracking-tight flex-1">タスク</h1>
        <button
          type="button"
          onClick={() => mutate("/api/tasks")}
          aria-label="再読み込み"
          className="w-9 h-9 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted transition-colors"
        >
          <IconRefresh className="w-4 h-4" />
        </button>
        <Button variant="primary" size="sm" onClick={() => setComposing(true)}>
          タスクを投げる
        </Button>
      </header>

      <main className="p-4 max-w-3xl mx-auto space-y-6">
        {isLoading && (
          <div className="flex justify-center py-10">
            <Spinner size="md" />
          </div>
        )}

        {error && (
          <div className="rounded-lg border border-danger/40 bg-danger-soft px-3 py-2 text-[13px] text-danger">
            タスクを取得できませんでした: {String(error.message ?? error)}
          </div>
        )}

        {!isLoading && !error && tasks.length === 0 && (
          <div className="text-center py-14">
            <div className="text-sm text-fg-muted">まだタスクはありません。</div>
            <div className="mt-1 text-[12.5px] text-fg-subtle">
              「このリポジトリに〜して」を投げると、空いているノードで実行され PR になります。
            </div>
            <Button variant="primary" size="md" className="mt-4" onClick={() => setComposing(true)}>
              最初のタスクを投げる
            </Button>
          </div>
        )}

        {active.length > 0 && (
          <Section title="進行中" count={active.length}>
            {active.map((t) => (
              <TaskCard key={t.id} task={t} busy={busy === t.id} onAct={act} />
            ))}
          </Section>
        )}
        {done.length > 0 && (
          <Section title="完了" count={done.length}>
            {done.map((t) => (
              <TaskCard key={t.id} task={t} busy={busy === t.id} onAct={act} />
            ))}
          </Section>
        )}
      </main>

      <NewTaskSheet
        open={composing}
        onClose={() => setComposing(false)}
        onSubmitted={() => {
          setComposing(false);
          void mutate("/api/tasks");
        }}
      />
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
}) {
  return (
    <section>
      <div className="text-xs font-medium text-fg-muted mb-2">
        {title}
        <span className="ml-1.5 text-fg-faint tabular-nums">{count}</span>
      </div>
      <div className="space-y-2">{children}</div>
    </section>
  );
}

function TaskCard({
  task,
  busy,
  onAct,
}: {
  task: TaskSummary;
  busy: boolean;
  onAct: (task: TaskSummary, action: "cancel" | "retry") => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const status = STATUS[task.status] ?? { tone: "neutral" as const, label: task.status };
  const cancellable = ACTIVE.includes(task.status);

  return (
    <div className="rounded-lg border border-line bg-surface-elev p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <Badge tone={status.tone}>{status.label}</Badge>
            <span className="text-[12px] text-fg-subtle font-mono truncate">
              {projectName(task.projectPath)}
            </span>
            <span className="text-[11px] text-fg-faint">{task.agent}</span>
            {task.nodeName && <span className="text-[11px] text-fg-faint">@{task.nodeName}</span>}
            {task.attempts > 1 && (
              <span className="text-[11px] text-fg-faint tabular-nums">
                {task.attempts}/{task.maxAttempts} 回目
              </span>
            )}
          </div>
          <button
            type="button"
            onClick={() => setExpanded((v) => !v)}
            className="mt-1 text-left text-sm text-fg hover:text-accent transition-colors block w-full truncate"
            title="全文を表示"
          >
            {task.title || task.prompt}
          </button>
          {expanded && (
            <pre className="mt-2 whitespace-pre-wrap text-[12.5px] text-fg-muted bg-surface-muted rounded-md p-2 max-h-64 overflow-y-auto scroll-thin">
              {task.prompt}
            </pre>
          )}
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          {busy && <Spinner size="sm" />}
          {cancellable ? (
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => onAct(task, "cancel")}
            >
              停止
            </Button>
          ) : (
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => onAct(task, "retry")}
            >
              再実行
            </Button>
          )}
        </div>
      </div>

      <div className="mt-2 flex items-center gap-3 flex-wrap text-[11.5px] text-fg-faint">
        <span>{when(task.createdAt)}</span>
        {task.branch && <span className="font-mono truncate">{task.branch}</span>}
        {task.prUrl && (
          <a
            href={task.prUrl}
            target="_blank"
            rel="noreferrer"
            className="text-accent hover:text-accent-hover"
          >
            PR を開く
          </a>
        )}
      </div>

      {task.error && (
        <div className="mt-2 rounded-md border border-danger/40 bg-danger-soft px-2 py-1.5 text-[12px] text-danger">
          {task.error}
        </div>
      )}
      {task.notes.length > 0 && (
        <ul className="mt-2 space-y-0.5">
          {task.notes.map((n) => (
            <li key={n} className="text-[12px] text-fg-subtle">
              · {n}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

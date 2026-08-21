import { useState } from "react";
import useSWR, { mutate } from "swr";
import { Link } from "@tanstack/react-router";
import { Badge, Button, Spinner } from "@/components/ui";
import { useToast } from "./Toast";
import { IconChevronRight, IconRefresh, IconServer } from "@/components/icons";

type NodeLabel = { key: string; value: string };

type NodeSummary = {
  id: string;
  name: string;
  status: "ready" | "notready" | "cordoned" | "lost";
  connected: boolean;
  cordoned: boolean;
  isLocal: boolean;
  version: string;
  os: string;
  arch: string;
  executors: string[];
  capacityCpuMillis: number;
  capacityMemMb: number;
  reservedCpuMillis: number;
  reservedMemMb: number;
  maxSessions: number;
  cpuUtil: number;
  memUtil: number;
  load1: number;
  labels: NodeLabel[];
  sessions: string[];
  cachedProjects: number;
  lastHeartbeatMs: number;
};

type NodesBody = { nodes: NodeSummary[]; clustered: boolean };

const fetcher = (url: string) => fetch(url).then((r) => r.json());

const STATUS: Record<
  NodeSummary["status"],
  { tone: "emerald" | "amber" | "red" | "neutral"; label: string }
> = {
  ready: { tone: "emerald", label: "Ready" },
  notready: { tone: "amber", label: "NotReady" },
  cordoned: { tone: "neutral", label: "Cordoned" },
  lost: { tone: "red", label: "Lost" },
};

function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

function gib(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GiB` : `${mb} MiB`;
}

function cores(millis: number): string {
  return `${(millis / 1000).toFixed(millis % 1000 === 0 ? 0 : 1)} コア`;
}

/** Horizontal fill showing how much of a node is already spoken for. */
function Meter({ value, label }: { value: number; label: string }) {
  const clamped = Math.max(0, Math.min(1, value));
  return (
    <div className="flex items-center gap-2">
      <div
        className="h-1.5 flex-1 rounded-full bg-surface-muted overflow-hidden"
        role="meter"
        aria-valuenow={Math.round(clamped * 100)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label}
      >
        <div
          className={[
            "h-full rounded-full transition-[width]",
            clamped > 0.9 ? "bg-danger" : clamped > 0.7 ? "bg-warn" : "bg-accent",
          ].join(" ")}
          style={{ width: `${clamped * 100}%` }}
        />
      </div>
      <span className="text-[10px] tabular-nums text-fg-faint w-9 text-right">{pct(clamped)}</span>
    </div>
  );
}

export function NodesPage() {
  const toast = useToast();
  const { data, isLoading } = useSWR<NodesBody>("/api/nodes", fetcher, {
    refreshInterval: 5000,
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [token, setToken] = useState<{ token: string; command: string } | null>(null);

  async function setCordon(node: NodeSummary, cordoned: boolean) {
    setBusy(node.id);
    try {
      const res = await fetch(`/api/nodes/${node.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ cordoned }),
      });
      if (!res.ok) throw new Error((await res.json()).error ?? res.statusText);
      await mutate("/api/nodes");
      toast({
        title: cordoned
          ? `${node.name} への新規割り当てを停止しました`
          : `${node.name} への割り当てを再開しました`,
        color: "success",
      });
    } catch (e) {
      toast({
        title: "変更に失敗しました",
        description: e instanceof Error ? e.message : String(e),
        color: "danger",
      });
    } finally {
      setBusy(null);
    }
  }

  async function issueToken() {
    setBusy("token");
    try {
      const res = await fetch("/api/join-tokens", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ttlSecs: 3600, uses: 1 }),
      });
      if (!res.ok) throw new Error((await res.json()).error ?? res.statusText);
      setToken(await res.json());
    } catch (e) {
      toast({
        title: "参加トークンの発行に失敗しました",
        description: e instanceof Error ? e.message : String(e),
        color: "danger",
      });
    } finally {
      setBusy(null);
    }
  }

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
        <h1 className="text-sm font-semibold tracking-tight flex-1">ノード</h1>
        <button
          type="button"
          onClick={() => mutate("/api/nodes")}
          aria-label="再読み込み"
          className="w-9 h-9 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted transition-colors"
        >
          <IconRefresh className="w-4 h-4" />
        </button>
      </header>

      <main className="p-4 max-w-3xl mx-auto space-y-4">
        {isLoading && (
          <div className="flex items-center gap-2 text-[12px] text-fg-subtle">
            <Spinner size="xs" /> 読み込み中…
          </div>
        )}

        {data && !data.clustered && (
          <p className="text-[12px] text-fg-subtle">
            このホストはスケジューラを持ちません（<code>--role node</code> で起動しています）。
          </p>
        )}

        {data?.clustered && data.nodes.length === 0 && (
          <p className="text-[12px] text-fg-subtle">接続中のノードがありません。</p>
        )}

        {data?.nodes.map((n) => {
          const status = STATUS[n.status];
          const cpuReserved =
            n.capacityCpuMillis > 0 ? n.reservedCpuMillis / n.capacityCpuMillis : 0;
          return (
            <section key={n.id} className="rounded-lg border border-line bg-surface-raised p-3">
              <div className="flex items-center gap-2">
                <IconServer className="w-4 h-4 text-fg-faint shrink-0" />
                <span className="text-[13px] font-medium truncate">{n.name}</span>
                <Badge tone={status.tone}>{status.label}</Badge>
                {n.isLocal && <Badge tone="blue">ローカル</Badge>}
                <span className="flex-1" />
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy === n.id || !n.connected}
                  onClick={() => setCordon(n, !n.cordoned)}
                >
                  {n.cordoned ? "割り当て再開" : "割り当て停止"}
                </Button>
              </div>

              <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-[11px]">
                <div>
                  <dt className="text-fg-faint">セッション</dt>
                  <dd className="tabular-nums">
                    {n.sessions.length}
                    {n.maxSessions > 0 ? ` / ${n.maxSessions}` : ""}
                  </dd>
                </div>
                <div>
                  <dt className="text-fg-faint">容量</dt>
                  <dd className="tabular-nums">
                    {cores(n.capacityCpuMillis)} · {gib(n.capacityMemMb)}
                  </dd>
                </div>
                <div className="col-span-2">
                  <dt className="text-fg-faint mb-1">CPU 予約</dt>
                  <dd>
                    <Meter value={cpuReserved} label={`${n.name} の CPU 予約率`} />
                  </dd>
                </div>
                <div className="col-span-2">
                  <dt className="text-fg-faint mb-1">CPU 実測</dt>
                  <dd>
                    <Meter value={n.cpuUtil} label={`${n.name} の CPU 使用率`} />
                  </dd>
                </div>
                <div>
                  <dt className="text-fg-faint">隔離</dt>
                  <dd>{n.executors.join(", ") || "process"}</dd>
                </div>
                <div>
                  <dt className="text-fg-faint">キャッシュ済み</dt>
                  <dd className="tabular-nums">{n.cachedProjects} プロジェクト</dd>
                </div>
                <div className="col-span-2 text-fg-faint">
                  {n.os}/{n.arch} · v{n.version}
                  {n.labels.length > 0 && (
                    <span className="ml-2 inline-flex flex-wrap gap-1 align-middle">
                      {n.labels.map((l) => (
                        <Badge key={l.key} tone="violet">
                          {l.key}={l.value}
                        </Badge>
                      ))}
                    </span>
                  )}
                </div>
              </dl>
            </section>
          );
        })}

        {data?.clustered && (
          <section className="rounded-lg border border-line p-3">
            <h2 className="text-[12px] font-medium">ノードを追加</h2>
            <p className="mt-1 text-[11px] text-fg-subtle">
              参加トークンを発行し、新しいマシンで表示されたコマンドを実行します。トークンは 1
              時間・1 回限り有効で、ここでしか表示されません。
            </p>
            <Button size="sm" className="mt-2" disabled={busy === "token"} onClick={issueToken}>
              参加トークンを発行
            </Button>
            {token && (
              <pre className="mt-2 p-2 rounded bg-surface-muted text-[10px] overflow-x-auto whitespace-pre-wrap break-all">
                {token.command}
              </pre>
            )}
          </section>
        )}
      </main>
    </div>
  );
}

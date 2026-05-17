"use client";

import { Input, Button, Spinner } from "@heroui/react";
import { useToast } from "@/components/Toast";
import useSWR, { mutate } from "swr";
import { useMemo, useState } from "react";
import { ProjectCard } from "@/components/ProjectCard";
import { SessionCard } from "@/components/SessionCard";
import { SessionPreviewModal } from "@/components/SessionPreviewModal";
import { SettingsSheet } from "@/components/SettingsSheet";
import {
  LaunchConfirmSheet,
  type LaunchOverrides,
} from "@/components/LaunchConfirmSheet";
import {
  DeleteConfirmSheet,
  type DeleteTarget,
} from "@/components/DeleteConfirmSheet";

type Project = {
  name: string;
  path: string;
  root: string;
  mtimeMs: number;
  isGit: boolean;
};
type TmuxSession = {
  name: string;
  path: string;
  createdAt: number;
  attached: boolean;
  cli: string;
  worktreePath: string;
  origPath: string;
};

const fetcher = (url: string) => fetch(url).then((r) => r.json());

export default function HomePage() {
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [launchTarget, setLaunchTarget] = useState<Project | null>(null);
  const [launching, setLaunching] = useState(false);

  const [previewName, setPreviewName] = useState<string | null>(null);

  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);
  const [deleting, setDeleting] = useState(false);

  const { data: projData, isLoading: projLoading } = useSWR<{
    projects: Project[];
  }>("/api/projects", fetcher);

  const { data: sessData, isLoading: sessLoading } = useSWR<{
    sessions: TmuxSession[];
  }>("/api/sessions", fetcher, { refreshInterval: 5000 });

  const filteredProjects = useMemo(() => {
    const all = projData?.projects ?? [];
    if (!query.trim()) return all;
    const q = query.toLowerCase();
    return all.filter(
      (p) =>
        p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q),
    );
  }, [projData, query]);

  const sessions = sessData?.sessions ?? [];

  const handleLaunch = async (o: LaunchOverrides) => {
    if (!launchTarget) return;
    setLaunching(true);
    try {
      const res = await fetch("/api/sessions", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          projectPath: launchTarget.path,
          cli: o.cli,
          skipPermissions: o.skipPermissions,
          extraArgs: o.extraArgs,
          createWorktree: o.createWorktree,
        }),
      });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      toast({
        title: "起動しました",
        description: json.name,
        color: "success",
      });
      setLaunchTarget(null);
      mutate("/api/sessions");
    } catch (e) {
      toast({
        title: "起動失敗",
        description: (e as Error).message,
        color: "danger",
      });
    } finally {
      setLaunching(false);
    }
  };

  const handleStopConfirm = async (deleteWorktree: boolean) => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      const url = `/api/sessions/${encodeURIComponent(deleteTarget.name)}${
        deleteWorktree ? "?deleteWorktree=1" : ""
      }`;
      const res = await fetch(url, { method: "DELETE" });
      const json = await res.json();
      if (!res.ok) throw new Error(json.error || `HTTP ${res.status}`);
      if (json.worktreeError) {
        toast({
          title: "停止 (worktree 削除失敗)",
          description: json.worktreeError,
          color: "warning",
        });
      } else {
        toast({ title: "停止しました", color: "success" });
      }
      setDeleteTarget(null);
      mutate("/api/sessions");
    } catch (e) {
      toast({
        title: "停止失敗",
        description: (e as Error).message,
        color: "danger",
      });
    } finally {
      setDeleting(false);
    }
  };

  return (
    <main className="min-h-screen bg-zinc-50 text-zinc-900 flex flex-col">
      <header className="safe-top sticky top-0 z-10 bg-white border-b border-zinc-200">
        <div className="px-4 py-3 flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <span className="text-xl font-bold tracking-tight">ccstart</span>
            <span className="text-xs text-zinc-500">claude / codex launcher</span>
          </div>
          <Button
            size="sm"
            variant="bordered"
            onPress={() => setSettingsOpen(true)}
            className="min-h-9 border-zinc-300 text-zinc-700"
            disableRipple
          >
            設定
          </Button>
        </div>
      </header>

      <section className="flex-1 px-4 py-4 safe-bottom space-y-6 max-w-2xl w-full mx-auto">
        <div>
          <div className="flex items-baseline justify-between mb-2">
            <h2 className="text-base font-bold text-zinc-900">
              起動中のセッション
            </h2>
            <span className="text-xs text-zinc-500">{sessions.length} 件</span>
          </div>
          {sessLoading ? (
            <div className="flex justify-center py-6">
              <Spinner size="sm" />
            </div>
          ) : sessions.length === 0 ? (
            <div className="text-center text-sm text-zinc-500 bg-white border border-dashed border-zinc-200 rounded-xl py-6">
              起動中のセッションはありません。
              <br />
              下のプロジェクトから「起動」をタップ。
            </div>
          ) : (
            <div className="space-y-2">
              {sessions.map((s) => (
                <SessionCard
                  key={s.name}
                  {...s}
                  onPreview={() => setPreviewName(s.name)}
                  onStop={() =>
                    setDeleteTarget({
                      name: s.name,
                      worktreePath: s.worktreePath,
                      origPath: s.origPath,
                    })
                  }
                />
              ))}
            </div>
          )}
        </div>

        <div>
          <div className="flex items-baseline justify-between mb-2">
            <h2 className="text-base font-bold text-zinc-900">プロジェクト</h2>
            <span className="text-xs text-zinc-500">
              {filteredProjects.length} 件
            </span>
          </div>
          <Input
            type="search"
            placeholder="プロジェクト名で検索"
            value={query}
            onValueChange={setQuery}
            variant="bordered"
            size="md"
            isClearable
            onClear={() => setQuery("")}
            classNames={{
              inputWrapper:
                "border-zinc-300 bg-white data-[hover=true]:border-zinc-400 data-[focus=true]:border-blue-500",
            }}
          />
          <div className="space-y-2 mt-3">
            {projLoading && (
              <div className="flex justify-center py-6">
                <Spinner size="sm" />
              </div>
            )}
            {!projLoading && filteredProjects.length === 0 && (
              <div className="text-center text-sm text-zinc-500 bg-white border border-dashed border-zinc-200 rounded-xl py-6">
                プロジェクトが見つかりません
              </div>
            )}
            {filteredProjects.map((p) => (
              <ProjectCard
                key={p.path}
                {...p}
                onSelect={() => setLaunchTarget(p)}
              />
            ))}
          </div>
        </div>
      </section>

      <SettingsSheet
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
      />

      <LaunchConfirmSheet
        isOpen={!!launchTarget}
        projectName={launchTarget?.name ?? ""}
        projectPath={launchTarget?.path ?? ""}
        isGit={!!launchTarget?.isGit}
        onClose={() => setLaunchTarget(null)}
        onLaunch={handleLaunch}
        launching={launching}
      />

      <DeleteConfirmSheet
        target={deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleStopConfirm}
        busy={deleting}
      />

      <SessionPreviewModal
        sessionName={previewName}
        isOpen={!!previewName}
        onClose={() => setPreviewName(null)}
      />
    </main>
  );
}

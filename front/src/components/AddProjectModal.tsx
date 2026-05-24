import { useState } from "react";
import { mutate } from "swr";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { Button, Input } from "@/components/ui";
import { useToast } from "./Toast";

type Tab = "clone" | "import";

type Props = {
  open: boolean;
  onClose: () => void;
};

export function AddProjectModal({ open, onClose }: Props) {
  const toast = useToast();
  const [tab, setTab] = useState<Tab>("clone");
  const [url, setUrl] = useState("");
  const [src, setSrc] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  const reset = () => {
    setUrl("");
    setSrc("");
    setName("");
    setBusy(false);
  };

  const submit = async () => {
    setBusy(true);
    try {
      const isClone = tab === "clone";
      const endpoint = isClone ? "/api/projects/clone" : "/api/projects/import";
      const body = isClone
        ? { url: url.trim(), name: name.trim() || undefined }
        : { src: src.trim(), name: name.trim() || undefined };
      if (isClone && !body.url) throw new Error("リポジトリ URL を入力してください");
      if (!isClone && !(body as { src: string }).src)
        throw new Error("コピー元ディレクトリを入力してください");

      const res = await fetch(endpoint, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      const json = await res.json();
      if (!res.ok && res.status !== 202) throw new Error(json.error ?? `HTTP ${res.status}`);

      toast({
        title: isClone ? "クローン中…" : "インポート中…",
        description: json.name,
        color: "info",
      });
      mutate("/api/projects");
      reset();
      onClose();
    } catch (e) {
      toast({ title: "失敗", description: (e as Error).message, color: "danger" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet open={open} onClose={onClose} maxWidth="lg">
      <SheetHeader
        title="プロジェクトを追加"
        subtitle={`~/.agent-start/projects/ 配下に${tab === "clone" ? "クローン" : "コピー"}します`}
        onClose={onClose}
      />
      <SheetBody>
        <div className="flex gap-1 mb-4">
          <TabButton active={tab === "clone"} onClick={() => setTab("clone")}>
            リポジトリをクローン
          </TabButton>
          <TabButton active={tab === "import"} onClick={() => setTab("import")}>
            ディレクトリをインポート
          </TabButton>
        </div>

        {tab === "clone" ? (
          <div className="space-y-3">
            <Input
              label="リポジトリ URL"
              placeholder="git@github.com:user/repo.git or https://…"
              value={url}
              onValueChange={setUrl}
            />
            <Input
              label="プロジェクト名 (任意)"
              placeholder="repo (省略時は URL から推測)"
              value={name}
              onValueChange={setName}
            />
          </div>
        ) : (
          <div className="space-y-3">
            <Input
              label="コピー元の絶対パス"
              placeholder="/Users/me/some-project"
              value={src}
              onValueChange={setSrc}
              description="指定したディレクトリの内容を再帰的にコピーします"
            />
            <Input
              label="プロジェクト名 (任意)"
              placeholder="dir (省略時はディレクトリ名)"
              value={name}
              onValueChange={setName}
            />
          </div>
        )}
      </SheetBody>
      <SheetFooter>
        <div className="flex-1" />
        <Button variant="secondary" size="md" onClick={onClose} disabled={busy}>
          キャンセル
        </Button>
        <Button variant="primary" size="md" loading={busy} onClick={submit}>
          {tab === "clone" ? "クローンを開始" : "インポートを開始"}
        </Button>
      </SheetFooter>
    </Sheet>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "px-3 py-1.5 rounded-md text-sm border transition-colors",
        active
          ? "border-accent bg-accent text-accent-fg"
          : "border-line bg-surface text-fg hover:bg-surface-muted",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

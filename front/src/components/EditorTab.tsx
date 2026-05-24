import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import CodeMirror, { EditorView, keymap } from "@uiw/react-codemirror";
import { markdown } from "@codemirror/lang-markdown";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { rust } from "@codemirror/lang-rust";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Spinner } from "@/components/ui";
import { useToast } from "@/components/Toast";
import { useTheme } from "@/components/ThemeProvider";

type FileResp = { content: string; sha: string; eol: string };

const fetcher = (url: string) => fetch(url).then((r) => r.json());

function langFor(path: string) {
  const lower = path.toLowerCase();
  if (lower.endsWith(".md") || lower.endsWith(".markdown")) return markdown();
  if (
    lower.endsWith(".ts") ||
    lower.endsWith(".tsx") ||
    lower.endsWith(".js") ||
    lower.endsWith(".jsx") ||
    lower.endsWith(".mjs") ||
    lower.endsWith(".cjs")
  )
    return javascript({ jsx: true, typescript: lower.endsWith(".ts") || lower.endsWith(".tsx") });
  if (lower.endsWith(".json")) return json();
  if (lower.endsWith(".rs")) return rust();
  return [];
}

type Props = {
  path: string;
  view: "edit" | "preview";
  onViewChange: (v: "edit" | "preview") => void;
  /** Reports buffer-vs-disk diff up so the tab strip can show a dot. */
  onDirtyChange: (dirty: boolean) => void;
};

export function EditorTab({ path, view, onViewChange, onDirtyChange }: Props) {
  const toast = useToast();
  const { resolved: resolvedTheme } = useTheme();

  const [content, setContent] = useState<string>("");
  const [baseSha, setBaseSha] = useState<string>("");
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const initialContentRef = useRef<string>("");

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setError(null);
    fetcher(`/api/fs/file?path=${encodeURIComponent(path)}`)
      .then((j: FileResp & { error?: string }) => {
        if (cancelled) return;
        if (j.error) {
          setError(j.error);
          setLoaded(true);
          return;
        }
        setContent(j.content ?? "");
        initialContentRef.current = j.content ?? "";
        setBaseSha(j.sha ?? "");
        setLoaded(true);
      })
      .catch((e) => {
        if (cancelled) return;
        setError((e as Error).message);
        setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  const dirty = content !== initialContentRef.current;
  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty, onDirtyChange]);

  const save = useCallback(async () => {
    if (saving) return;
    setSaving(true);
    try {
      const res = await fetch("/api/fs/file", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path, content, baseSha }),
      });
      const j = await res.json();
      if (res.status === 409) {
        toast({
          title: "ファイルが変更されています",
          description: "再読み込みしてから保存し直してください",
          color: "warning",
        });
        return;
      }
      if (!res.ok) throw new Error(j.error ?? `HTTP ${res.status}`);
      setBaseSha(j.sha ?? "");
      initialContentRef.current = content;
      toast({ title: "保存しました", color: "success" });
    } catch (e) {
      toast({ title: "保存失敗", description: (e as Error).message, color: "danger" });
    } finally {
      setSaving(false);
    }
  }, [content, baseSha, path, toast, saving]);

  // Bind Cmd/Ctrl+S inside the editor. Outside the editor (when previewing),
  // listen on the document so the shortcut still works.
  useEffect(() => {
    if (view !== "preview") return;
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [view, save]);

  const saveKeymap = useMemo(
    () =>
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            save();
            return true;
          },
        },
      ]),
    [save],
  );

  const extensions = useMemo(
    () => [langFor(path), saveKeymap, EditorView.lineWrapping],
    [path, saveKeymap],
  );

  const isMarkdown = /\.(md|markdown)$/i.test(path);

  return (
    <div className="flex-1 min-h-0 flex flex-col bg-app">
      <div className="px-3 py-1.5 border-b border-line bg-surface flex items-center gap-2">
        <div className="text-[11px] font-mono text-fg-subtle truncate flex-1">{path}</div>
        {dirty && <span className="text-[10px] text-warn">未保存</span>}
        <div className="inline-flex rounded-md border border-line overflow-hidden">
          <ViewToggle active={view === "edit"} onClick={() => onViewChange("edit")}>
            編集
          </ViewToggle>
          <ViewToggle
            active={view === "preview"}
            onClick={() => onViewChange("preview")}
            disabled={!isMarkdown}
            title={!isMarkdown ? "プレビューは Markdown のみ" : undefined}
          >
            プレビュー
          </ViewToggle>
        </div>
        <button
          type="button"
          onClick={save}
          disabled={!dirty || saving}
          className={[
            "h-7 px-2.5 text-[12px] rounded-md border transition-colors",
            !dirty || saving
              ? "border-line text-fg-faint cursor-not-allowed"
              : "border-accent bg-accent text-accent-fg hover:opacity-90",
          ].join(" ")}
          title="保存 (⌘S)"
        >
          {saving ? "保存中…" : "保存"}
        </button>
      </div>

      <div className="flex-1 min-h-0">
        {!loaded ? (
          <div className="h-full flex items-center justify-center">
            <Spinner size="md" />
          </div>
        ) : error ? (
          <div className="p-4 text-sm text-danger">読み込めません: {error}</div>
        ) : view === "preview" && isMarkdown ? (
          <div className="md-preview p-6 overflow-y-auto h-full max-w-3xl mx-auto text-sm">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        ) : view === "preview" ? (
          <div className="p-4 text-sm text-fg-subtle">
            このファイルはプレビューに対応していません。
          </div>
        ) : (
          <CodeMirror
            value={content}
            onChange={(v) => setContent(v)}
            extensions={extensions}
            theme={resolvedTheme === "dark" ? "dark" : "light"}
            height="100%"
            className="h-full"
            basicSetup={{ highlightActiveLine: true, lineNumbers: true }}
          />
        )}
      </div>
    </div>
  );
}

function ViewToggle({
  active,
  onClick,
  disabled,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      title={title}
      className={[
        "px-2.5 h-7 text-[12px] transition-colors",
        active
          ? "bg-surface-muted text-fg"
          : disabled
            ? "text-fg-faint cursor-not-allowed"
            : "bg-surface text-fg-subtle hover:bg-surface-muted",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

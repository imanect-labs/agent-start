import { useCallback, useEffect, useRef, useState } from "react";
import { useToast } from "@/components/Toast";
import { useMediaQuery } from "@/lib/useMediaQuery";
import type { ChatModelInfo } from "@/components/ChatView";
import { prettyModel, type OutgoingImage } from "@/lib/chat-types";

/** Per-device send-key preference (U4). Mirrored by SettingsPage. */
export const SEND_KEY_STORAGE = "agent-start:chat:sendKey";
export type SendKey = "enter" | "ctrlEnter";

export function readSendKey(): SendKey {
  if (typeof window === "undefined") return "enter";
  return window.localStorage.getItem(SEND_KEY_STORAGE) === "ctrlEnter" ? "ctrlEnter" : "enter";
}

const MAX_IMAGES = 4;
const MAX_BYTES = 5 * 1024 * 1024;
const ALLOWED = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const MAX_EDGE = 1568; // long-edge cap for the full-resolution send (U8)
const THUMB_EDGE = 96;

type Pending = OutgoingImage & { id: string };

export function ChatComposer({
  models,
  currentModel,
  onSend,
  onInterrupt,
  onSetModel,
  generating,
  disabled,
  dead,
}: {
  models: ChatModelInfo[];
  currentModel: string | null;
  /** Returns false if the transport wasn't ready (draft is then preserved). */
  onSend: (text: string, images: OutgoingImage[]) => boolean;
  onInterrupt: () => void;
  onSetModel: (model: string) => void;
  generating: boolean;
  disabled: boolean;
  dead: boolean;
}) {
  const toast = useToast();
  const [text, setText] = useState("");
  const [images, setImages] = useState<Pending[]>([]);
  const [sendKey, setSendKey] = useState<SendKey>(readSendKey);
  const [dragOver, setDragOver] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const coarse = useMediaQuery("(pointer: coarse)");

  // Keep the send-key preference live if changed in another tab / Settings.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === SEND_KEY_STORAGE) setSendKey(readSendKey());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // Auto-grow the textarea up to a cap.
  useEffect(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = `${Math.min(ta.scrollHeight, 200)}px`;
  }, [text]);

  const canSend = (text.trim().length > 0 || images.length > 0) && !disabled && !generating;

  const submit = useCallback(() => {
    if (!canSend) return;
    const ok = onSend(
      text.trim(),
      images.map(({ id: _id, ...rest }) => rest),
    );
    // Preserve the draft if the socket wasn't ready, so input isn't lost on
    // a connection race.
    if (!ok) {
      toast({ title: "送信できませんでした（接続待ち）", color: "warning" });
      return;
    }
    setText("");
    setImages([]);
  }, [canSend, onSend, text, images, toast]);

  const addFiles = useCallback(
    async (files: FileList | File[]) => {
      const list = Array.from(files);
      for (const file of list) {
        if (images.length >= MAX_IMAGES) {
          toast({ title: `画像は最大 ${MAX_IMAGES} 枚までです`, color: "warning" });
          break;
        }
        if (!ALLOWED.includes(file.type)) {
          toast({ title: "対応していない画像形式です", description: file.type, color: "danger" });
          continue;
        }
        if (file.size > MAX_BYTES) {
          toast({ title: "画像が大きすぎます (上限 5MB)", color: "danger" });
          continue;
        }
        try {
          const img = await processImage(file);
          setImages((prev) => (prev.length >= MAX_IMAGES ? prev : [...prev, img]));
        } catch {
          toast({ title: "画像の処理に失敗しました", color: "danger" });
        }
      }
    },
    [images.length, toast],
  );

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // IME guard (U4): never submit while composing Japanese, etc.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (e.key !== "Enter") return;
    if (coarse) return; // touch: Enter is newline; send via button.
    if (sendKey === "enter") {
      if (!e.shiftKey && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        submit();
      }
    } else if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      submit();
    }
  };

  const onPaste = (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files);
    if (files.length > 0) {
      e.preventDefault();
      void addFiles(files);
    }
  };

  return (
    <div
      className={[
        "border-t border-line bg-surface px-3 py-2.5 sm:px-4",
        dragOver ? "ring-2 ring-accent/50 ring-inset" : "",
      ].join(" ")}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragOver(false);
        if (e.dataTransfer.files.length) void addFiles(e.dataTransfer.files);
      }}
    >
      {images.length > 0 && (
        <div className="flex flex-wrap gap-2 mb-2">
          {images.map((img) => (
            <div key={img.id} className="relative group">
              <img
                src={img.thumb}
                alt="添付"
                className="h-14 w-14 rounded-md object-cover border border-line"
              />
              <button
                type="button"
                aria-label="画像を削除"
                onClick={() => setImages((prev) => prev.filter((p) => p.id !== img.id))}
                className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-surface-elev border border-line text-fg-subtle hover:text-fg flex items-center justify-center text-[11px] shadow-sm"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      <textarea
        ref={taRef}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        rows={1}
        placeholder={dead ? "メッセージを送信して会話を再開…" : "メッセージを入力…"}
        // 16px on touch devices stops iOS Safari from auto-zooming the page
        // when the field gains focus (it zooms any input < 16px).
        className="w-full resize-none bg-transparent text-[14px] [@media(pointer:coarse)]:text-[16px] text-fg placeholder:text-fg-faint outline-none leading-relaxed max-h-[200px]"
      />

      <div className="flex items-center gap-2 mt-2">
        <button
          type="button"
          aria-label="画像を添付"
          title="画像を添付"
          onClick={() => fileRef.current?.click()}
          className="w-9 h-9 inline-flex items-center justify-center rounded-md text-fg-subtle hover:text-fg hover:bg-surface-muted transition-colors"
        >
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" className="w-4 h-4">
            <path
              strokeWidth="1.6"
              strokeLinecap="round"
              d="M13.5 7l-5 5a2 2 0 102.8 2.8l5-5a3.5 3.5 0 10-5-5l-5.3 5.3a5 5 0 107 7l4.5-4.5"
            />
          </svg>
        </button>
        <input
          ref={fileRef}
          type="file"
          accept={ALLOWED.join(",")}
          multiple
          className="hidden"
          onChange={(e) => {
            if (e.target.files) void addFiles(e.target.files);
            e.target.value = "";
          }}
        />

        <ModelPicker
          models={models}
          current={currentModel}
          onSelect={onSetModel}
          disabled={disabled}
        />

        <div className="ml-auto flex items-center gap-2">
          <SendKeyHint sendKey={sendKey} coarse={coarse} />
          {generating ? (
            <button
              type="button"
              onClick={onInterrupt}
              className="inline-flex items-center gap-1.5 h-9 px-3.5 rounded-lg bg-surface-muted text-fg border border-line-strong hover:bg-surface-elev transition-colors text-[13px] font-medium"
            >
              <span className="w-2.5 h-2.5 rounded-[2px] bg-fg" />
              停止
            </button>
          ) : (
            <button
              type="button"
              onClick={submit}
              disabled={!canSend}
              className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-[13px] font-medium"
            >
              送信
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function SendKeyHint({ sendKey, coarse }: { sendKey: SendKey; coarse: boolean }) {
  if (coarse) return null;
  return (
    <span className="hidden sm:inline text-[10px] text-fg-faint">
      {sendKey === "enter" ? "Enter で送信 / Shift+Enter 改行" : "Ctrl+Enter で送信"}
    </span>
  );
}

function ModelPicker({
  models,
  current,
  onSelect,
  disabled,
}: {
  models: ChatModelInfo[];
  current: string | null;
  onSelect: (model: string) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  if (models.length === 0) return null;
  // `current` is usually the resolved id from system:init (e.g.
  // "claude-opus-4-7"); show a humanized name and match menu items by family.
  const isActive = (id: string) => current != null && (current === id || current.includes(id));
  const label = prettyModel(current);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-1 h-7 px-2 rounded-md bg-surface-muted border border-line text-[12px] text-fg-muted hover:text-fg hover:border-line-strong disabled:opacity-40 transition-colors"
        title="モデルを切り替え"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-accent" />
        {label}
      </button>
      {open && (
        <div className="absolute bottom-full left-0 mb-1.5 z-30 min-w-[160px] bg-surface-elev border border-line rounded-lg shadow-lg py-1">
          {models.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => {
                setOpen(false);
                if (!isActive(m.id)) onSelect(m.id);
              }}
              className={[
                "w-full text-left px-3 py-1.5 text-[13px] flex items-center gap-2",
                isActive(m.id) ? "text-fg" : "text-fg-muted hover:bg-surface-muted",
              ].join(" ")}
            >
              <span
                className={[
                  "w-1.5 h-1.5 rounded-full",
                  isActive(m.id) ? "bg-accent" : "bg-transparent",
                ].join(" ")}
              />
              {m.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Downscale + re-encode an image, returning the full payload + a thumbnail. */
async function processImage(file: File): Promise<Pending> {
  const dataUrl = await readAsDataUrl(file);
  const img = await loadImage(dataUrl);
  const full = downscale(img, MAX_EDGE, file.type);
  const thumb = downscale(img, THUMB_EDGE, "image/jpeg");
  const mediaType = full.startsWith("data:image/png") ? "image/png" : "image/jpeg";
  const base64 = full.split(",")[1] ?? "";
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    mediaType,
    data: base64,
    thumb,
  };
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(file);
  });
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

function downscale(img: HTMLImageElement, maxEdge: number, mime: string): string {
  const { width, height } = img;
  const scale = Math.min(1, maxEdge / Math.max(width, height));
  const w = Math.max(1, Math.round(width * scale));
  const h = Math.max(1, Math.round(height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return img.src;
  ctx.drawImage(img, 0, 0, w, h);
  // PNG keeps crisp UI screenshots; everything else re-encodes to JPEG.
  const outMime = mime === "image/png" ? "image/png" : "image/jpeg";
  return canvas.toDataURL(outMime, outMime === "image/jpeg" ? 0.85 : undefined);
}

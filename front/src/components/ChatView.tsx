import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Spinner } from "@/components/ui";
import { ChatComposer } from "@/components/chat/ChatComposer";
import { ChatMessage, DraftView, TypingIndicator } from "@/components/chat/ChatMessage";
import { useChatSocket } from "@/lib/useChatSocket";
import { prettyModel } from "@/lib/chat-types";

export type ChatModelInfo = { id: string; label: string };

/**
 * Headless-`claude` chat surface (#34). Thin header + auto-following message
 * list + bottom composer (U1). A dead conversation is revived transparently
 * on the next send (U5), so there is no separate "restart" button here.
 */
export function ChatView({
  sessionName,
  cwd,
  models,
  defaultModel,
}: {
  sessionName: string;
  cwd: string;
  models: ChatModelInfo[];
  defaultModel: string | null;
}) {
  const chat = useChatSocket(sessionName);
  const currentModel = chat.model ?? defaultModel;
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  // Follow new content only when the user is already at the bottom (U1):
  // reading scrollback up top is never yanked back down.
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && atBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [chat.messages, chat.draft, chat.generating]);

  const isEmpty = chat.messages.length === 0 && !chat.draft && !chat.generating;
  const dead = chat.lifecycle === "dead";

  return (
    <div className="flex flex-col h-full min-h-0 bg-app">
      <Header
        model={currentModel}
        connection={chat.connection}
        lifecycle={chat.lifecycle}
        onReconnect={chat.reconnect}
      />

      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="flex-1 min-h-0 overflow-y-auto scroll-thin"
      >
        {chat.connection === "connecting" && chat.messages.length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-subtle">
            <div className="flex items-center gap-2 text-sm">
              <Spinner size="sm" />
              接続中…
            </div>
          </div>
        ) : isEmpty ? (
          <EmptyState model={currentModel} cwd={cwd} />
        ) : (
          <div className="mx-auto max-w-3xl w-full px-3 sm:px-5 py-4 space-y-4">
            {chat.messages.map((m) => (
              <ChatMessage key={m.seq} msg={m} toolResults={chat.toolResults} />
            ))}
            {chat.generating && !chat.draft && <TypingIndicator />}
            {chat.draft && <DraftView draft={chat.draft} />}
          </div>
        )}
      </div>

      {chat.error && (
        <div className="mx-3 sm:mx-5 mb-2 rounded-lg border border-danger/40 bg-danger-soft px-3 py-2 text-[13px] text-danger">
          {chat.error}
        </div>
      )}
      {dead && !chat.error && (
        <div className="mx-3 sm:mx-5 mb-2 rounded-lg border border-line-strong bg-surface-muted px-3 py-2 text-[12.5px] text-fg-muted">
          会話は停止しています。メッセージを送信すると <span className="font-mono">--resume</span>{" "}
          で再開します。
        </div>
      )}

      <ChatComposer
        models={models}
        currentModel={currentModel}
        onSend={chat.send}
        onInterrupt={chat.interrupt}
        onSetModel={chat.setModel}
        generating={chat.generating}
        disabled={chat.connection !== "open"}
        dead={dead}
      />
    </div>
  );
}

function Header({
  model,
  connection,
  lifecycle,
  onReconnect,
}: {
  model: string | null;
  connection: "connecting" | "open" | "closed";
  lifecycle: "running" | "dead" | "switching" | "unknown";
  onReconnect: () => void;
}) {
  const dotClass =
    connection === "open"
      ? lifecycle === "dead"
        ? "bg-warn"
        : "bg-success"
      : connection === "connecting"
        ? "bg-warn"
        : "bg-danger";
  const statusText =
    lifecycle === "switching"
      ? "モデル切替中…"
      : connection === "open"
        ? lifecycle === "dead"
          ? "停止中"
          : "接続中"
        : connection === "connecting"
          ? "接続中…"
          : "切断";

  return (
    <div className="flex items-center gap-2 px-3 sm:px-4 h-9 border-b border-line bg-surface/80 backdrop-blur-sm shrink-0">
      <span className={["inline-block w-1.5 h-1.5 rounded-full", dotClass].join(" ")} />
      <span className="text-[12px] text-fg-muted font-mono">{prettyModel(model)}</span>
      <span className="text-[11px] text-fg-faint ml-1">{statusText}</span>
      {connection === "closed" && (
        <button
          type="button"
          onClick={onReconnect}
          className="ml-auto text-[11px] text-accent hover:text-accent-hover"
        >
          再接続
        </button>
      )}
    </div>
  );
}

function EmptyState({ model, cwd }: { model: string | null; cwd: string }) {
  const tail = cwd ? cwd.split("/").filter(Boolean).slice(-2).join("/") : "";
  const [chip, setChip] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setChip(true), 50);
    return () => clearTimeout(t);
  }, []);
  return (
    <div className="h-full flex items-center justify-center">
      <div
        className={[
          "text-center px-6 transition-all duration-300",
          chip ? "opacity-100 translate-y-0" : "opacity-0 translate-y-1",
        ].join(" ")}
      >
        <div className="mx-auto w-12 h-12 rounded-xl bg-surface-muted border border-line flex items-center justify-center text-fg-subtle text-lg">
          ◇
        </div>
        <div className="mt-3 text-sm font-medium text-fg">{prettyModel(model)} とチャット</div>
        {tail && <div className="mt-1 text-[12px] text-fg-faint font-mono">…/{tail}</div>}
        <div className="mt-2 text-[12px] text-fg-subtle">
          メッセージを送って会話を始めましょう。
        </div>
      </div>
    </div>
  );
}

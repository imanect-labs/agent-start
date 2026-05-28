import { useState } from "react";
import { Spinner } from "@/components/ui";
import { ChatMarkdown } from "@/components/chat/ChatMarkdown";
import type {
  AssistantBlock,
  Draft,
  RenderMsg,
  ToolResult,
  UserContentBlock,
} from "@/lib/chat-types";

/** One committed message — user bubble (right) or assistant content (full width, U2). */
export function ChatMessage({
  msg,
  toolResults,
}: {
  msg: RenderMsg;
  toolResults: Map<string, ToolResult>;
}) {
  if (msg.role === "user") {
    return <UserMessage blocks={msg.blocks} />;
  }
  return (
    <div className="w-full space-y-1.5">
      {msg.blocks.map((b, i) => (
        <AssistantBlockView key={i} block={b} toolResults={toolResults} />
      ))}
    </div>
  );
}

function UserMessage({ blocks }: { blocks: UserContentBlock[] }) {
  const text = blocks
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("\n");
  const images = blocks.filter(
    (b): b is { type: "image"; thumb?: string | null } => b.type === "image",
  );
  return (
    <div className="flex justify-end">
      <div className="max-w-[85%] rounded-2xl rounded-br-sm bg-accent-soft border border-accent/15 px-3.5 py-2">
        {images.length > 0 && (
          <div className="flex flex-wrap gap-1.5 mb-1.5">
            {images.map((img, i) =>
              img.thumb ? (
                <img
                  key={i}
                  src={img.thumb}
                  alt="添付画像"
                  className="h-16 w-16 rounded object-cover border border-line"
                />
              ) : (
                <span
                  key={i}
                  className="inline-flex items-center gap-1 h-7 px-2 rounded bg-surface-muted text-2xs text-fg-subtle border border-line"
                >
                  🖼 画像
                </span>
              ),
            )}
          </div>
        )}
        {text && <div className="whitespace-pre-wrap break-words text-sm text-fg">{text}</div>}
      </div>
    </div>
  );
}

function AssistantBlockView({
  block,
  toolResults,
}: {
  block: AssistantBlock;
  toolResults: Map<string, ToolResult>;
}) {
  if (block.type === "text") {
    return <ChatMarkdown text={block.text} />;
  }
  if (block.type === "thinking") {
    return <ThinkingBlock text={block.thinking} />;
  }
  if (block.type === "tool_use") {
    return <ToolCard name={block.name} input={block.input} result={toolResults.get(block.id)} />;
  }
  return null;
}

function ThinkingBlock({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  if (!text.trim()) return null;
  return (
    <div className="my-1">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-1 text-2xs text-fg-faint hover:text-fg-subtle transition-colors"
      >
        <Chevron open={open} />
        思考
      </button>
      {open && (
        <div className="mt-1 pl-3 border-l border-line text-xs italic text-fg-subtle whitespace-pre-wrap break-words">
          {text}
        </div>
      )}
    </div>
  );
}

function ToolCard({ name, input, result }: { name: string; input?: unknown; result?: ToolResult }) {
  const [open, setOpen] = useState(false);
  const running = !result;
  const inputStr = formatInput(input);
  return (
    <div
      className={[
        "my-1.5 rounded-lg border text-xs overflow-hidden",
        result?.isError ? "border-danger/40" : "border-line",
      ].join(" ")}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2 px-3 py-1.5 bg-surface-muted/60 hover:bg-surface-muted transition-colors text-left"
      >
        <Chevron open={open} />
        <span className="font-mono text-xs text-fg">{name}</span>
        <span className="ml-auto inline-flex items-center gap-1.5 text-fg-faint">
          {running ? (
            <>
              <Spinner size="sm" />
              <span className="text-2xs">実行中</span>
            </>
          ) : result?.isError ? (
            <span className="text-2xs text-danger">エラー</span>
          ) : (
            <span className="text-2xs text-success">完了</span>
          )}
        </span>
      </button>
      {open && (
        <div className="px-3 py-2 space-y-2 bg-surface-sunken/40">
          {inputStr && (
            <div>
              <div className="text-2xs uppercase tracking-wide text-fg-faint mb-0.5">入力</div>
              <pre className="overflow-x-auto scroll-thin text-xs font-mono text-fg-muted whitespace-pre-wrap break-words">
                {inputStr}
              </pre>
            </div>
          )}
          {result && (
            <div>
              <div className="text-2xs uppercase tracking-wide text-fg-faint mb-0.5">結果</div>
              <pre
                className={[
                  "overflow-x-auto scroll-thin text-xs font-mono whitespace-pre-wrap break-words max-h-64",
                  result.isError ? "text-danger" : "text-fg-muted",
                ].join(" ")}
              >
                {result.content || "(空)"}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** The live, streaming-in block (text or thinking) shown before it commits. */
export function DraftView({ draft }: { draft: Draft }) {
  if (draft.kind === "thinking") {
    return (
      <div className="my-1 pl-3 border-l border-line text-xs italic text-fg-subtle whitespace-pre-wrap break-words">
        {draft.text}
        <Caret />
      </div>
    );
  }
  // Render Markdown live as tokens stream in (the hook coalesces deltas to
  // animation frames so this re-parse stays smooth). The committed block
  // replaces this draft seamlessly when the content block finishes.
  return (
    <div className="w-full">
      <ChatMarkdown text={draft.text} />
    </div>
  );
}

export function TypingIndicator() {
  return (
    <div className="flex items-center gap-1 py-1" aria-label="生成中">
      <Dot delay="0ms" />
      <Dot delay="150ms" />
      <Dot delay="300ms" />
    </div>
  );
}

function Dot({ delay }: { delay: string }) {
  return (
    <span
      className="inline-block w-1.5 h-1.5 rounded-full bg-fg-faint animate-bounce"
      style={{ animationDelay: delay, animationDuration: "1s" }}
    />
  );
}

function Caret() {
  return (
    <span className="inline-block w-1.5 h-3.5 ml-0.5 align-middle bg-fg-faint animate-pulse" />
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="currentColor"
      className={["w-3 h-3 shrink-0 transition-transform", open ? "rotate-90" : ""].join(" ")}
    >
      <path d="M7 5l6 5-6 5V5z" />
    </svg>
  );
}

function formatInput(input: unknown): string {
  if (input == null) return "";
  if (typeof input === "string") return input;
  try {
    return JSON.stringify(input, null, 2);
  } catch {
    return String(input);
  }
}

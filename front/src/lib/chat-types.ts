/**
 * Wire types for the chat WebSocket (#34).
 *
 * The server forwards Claude stream-json events verbatim (decision 3) plus
 * a few host-synthesized envelopes (`user_input`, `chat_status`,
 * `chat_error`). Committed messages carry a monotonic `_seq` so the client
 * can dedupe replayed transcript against live events.
 */

export type UserContentBlock =
  | { type: "text"; text: string }
  | { type: "image"; media_type?: string; thumb?: string | null };

export type AssistantBlock =
  | { type: "thinking"; thinking: string }
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input?: unknown };

export type ToolResultBlock = {
  type: "tool_result";
  tool_use_id: string;
  content?: unknown;
  is_error?: boolean;
};

/** A committed, rendered message (one per `_seq`). */
export type RenderMsg =
  | { seq: number; role: "user"; blocks: UserContentBlock[] }
  | { seq: number; role: "assistant"; blocks: AssistantBlock[] };

/** Result of a tool call, keyed by the originating `tool_use` id. */
export type ToolResult = { content: string; isError: boolean };

/** A live, not-yet-committed block being streamed token-by-token. */
export type Draft = { kind: "thinking" | "text"; text: string };

export type Connection = "connecting" | "open" | "closed";
export type Lifecycle = "running" | "dead" | "switching" | "unknown";

/**
 * Humanize a Claude model id for display. The picker sends short aliases
 * (`opus`/`sonnet`/`haiku`) but `system:init` reports the resolved id like
 * `claude-opus-4-7` or `claude-haiku-4-5-20251001`; show that as "Opus 4.7"
 * / "Haiku 4.5" so the badge is meaningful, not a raw slug.
 */
export function prettyModel(id: string | null | undefined): string {
  if (!id) return "Claude";
  let s = id.replace(/^claude-/, "").replace(/-\d{8}$/, ""); // drop date suffix
  const parts = s.split("-");
  const family = parts.shift() ?? s;
  const fam = family.charAt(0).toUpperCase() + family.slice(1);
  const ver = parts.filter((p) => /^\d+$/.test(p)).join(".");
  const ctx = parts.find((p) => /^\d+m$/i.test(p)); // e.g. "1m" → 1M context
  return [fam, ver, ctx?.toUpperCase()].filter(Boolean).join(" ");
}

/** One image attachment ready to send. */
export type OutgoingImage = {
  mediaType: string;
  /** Full base64 payload (no data: prefix) sent to Claude. */
  data: string;
  /** Small thumbnail data URL shown in the transcript. */
  thumb: string;
};

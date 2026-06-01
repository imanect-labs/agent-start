import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import type {
  AskQuestion,
  AssistantBlock,
  Connection,
  Draft,
  Lifecycle,
  OutgoingImage,
  PermissionRequest,
  RenderMsg,
  ToolResult,
  UserContentBlock,
} from "@/lib/chat-types";

/**
 * Drives one chat conversation over `/ws/chat?session=<name>`.
 *
 * Responsibilities:
 *  - reconnect with backoff (host restart / network blip), replaying the
 *    transcript idempotently (committed messages are upserted by `_seq`);
 *  - reconstruct token streaming from `stream_event` deltas into a live
 *    `draft`, replaced by the committed `assistant` message when it lands;
 *  - track connection + lifecycle (running/dead) and the current model;
 *  - expose `send` / `interrupt` / `setModel` actions.
 */

type State = {
  bySeq: Map<number, RenderMsg>;
  messages: RenderMsg[];
  toolResults: Map<string, ToolResult>;
  draft: Draft | null;
  generating: boolean;
  model: string | null;
  lifecycle: Lifecycle;
  error: string | null;
  /** Pending interactive permission requests (AskUserQuestion / plan), in
   *  arrival order, keyed by `requestId` (#95). */
  perms: PermissionRequest[];
  /** Current permission mode reported by the CLI (`"plan"`, `"default"`…). */
  permissionMode: string | null;
};

type Action =
  | { t: "commit"; msg: RenderMsg }
  | { t: "toolResult"; id: string; result: ToolResult }
  | { t: "draftStart"; kind: "thinking" | "text" }
  | { t: "draftAppend"; text: string }
  | { t: "draftClear" }
  | { t: "generating"; on: boolean }
  | { t: "model"; model: string | null }
  | { t: "lifecycle"; lifecycle: Lifecycle }
  | { t: "error"; message: string | null }
  | { t: "permAdd"; req: PermissionRequest }
  | { t: "permResolve"; requestId: string }
  | { t: "permClear" }
  | { t: "permissionMode"; mode: string | null }
  | { t: "resetTransient" };

function reducer(state: State, action: Action): State {
  switch (action.t) {
    case "commit": {
      const bySeq = new Map(state.bySeq);
      bySeq.set(action.msg.seq, action.msg);
      const messages = Array.from(bySeq.values()).sort((a, b) => a.seq - b.seq);
      // A committed assistant block supersedes the live draft for it.
      const draft = action.msg.role === "assistant" ? null : state.draft;
      return { ...state, bySeq, messages, draft };
    }
    case "toolResult": {
      const toolResults = new Map(state.toolResults);
      toolResults.set(action.id, action.result);
      return { ...state, toolResults };
    }
    case "draftStart":
      return { ...state, draft: { kind: action.kind, text: "" }, generating: true };
    case "draftAppend": {
      const cur = state.draft ?? { kind: "text" as const, text: "" };
      return { ...state, draft: { ...cur, text: cur.text + action.text }, generating: true };
    }
    case "draftClear":
      return { ...state, draft: null };
    case "generating":
      return { ...state, generating: action.on, draft: action.on ? state.draft : null };
    case "model":
      return { ...state, model: action.model };
    case "lifecycle":
      return { ...state, lifecycle: action.lifecycle };
    case "error":
      return { ...state, error: action.message };
    case "permAdd": {
      // Dedupe by requestId so a reconnect replay doesn't double-add.
      if (state.perms.some((p) => p.requestId === action.req.requestId)) return state;
      return { ...state, perms: [...state.perms, action.req] };
    }
    case "permResolve": {
      const perms = state.perms.filter((p) => p.requestId !== action.requestId);
      return perms.length === state.perms.length ? state : { ...state, perms };
    }
    case "permClear":
      return state.perms.length === 0 ? state : { ...state, perms: [] };
    case "permissionMode":
      return { ...state, permissionMode: action.mode };
    case "resetTransient":
      return { ...state, draft: null, generating: false };
    default:
      return state;
  }
}

const initialState: State = {
  bySeq: new Map(),
  messages: [],
  toolResults: new Map(),
  draft: null,
  generating: false,
  model: null,
  lifecycle: "unknown",
  error: null,
  perms: [],
  permissionMode: null,
};

/** Parse a `chat_permission` envelope into a typed PermissionRequest (#95). */
function parsePermission(env: Record<string, unknown>): PermissionRequest | null {
  const requestId = env.request_id as string | undefined;
  const tool = env.tool as string | undefined;
  const input = env.input as Record<string, unknown> | undefined;
  if (!requestId || !tool) return null;
  if (tool === "AskUserQuestion") {
    const questions = (input?.questions as AskQuestion[] | undefined) ?? [];
    if (questions.length === 0) return null;
    return { requestId, tool: "AskUserQuestion", questions };
  }
  if (tool === "ExitPlanMode") {
    const plan = typeof input?.plan === "string" ? (input.plan as string) : "";
    return { requestId, tool: "ExitPlanMode", plan };
  }
  return null;
}

/** Flatten a tool_result `content` (string | block[]) into display text. */
function toolResultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((b) =>
        b && typeof b === "object" && "text" in b ? String((b as { text: unknown }).text) : "",
      )
      .join("");
  }
  return content == null ? "" : JSON.stringify(content);
}

export function useChatSocket(sessionName: string) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const [connection, setConnection] = useState<Connection>("connecting");
  const [attempt, setAttempt] = useState(0);
  const wsRef = useRef<WebSocket | null>(null);

  // Coalesce token deltas to one dispatch per animation frame so live
  // Markdown rendering stays smooth even at high token rates.
  const draftBufRef = useRef("");
  const rafRef = useRef<number | null>(null);
  const flushDraft = useCallback(() => {
    rafRef.current = null;
    const text = draftBufRef.current;
    draftBufRef.current = "";
    if (text) dispatch({ t: "draftAppend", text });
  }, []);
  const appendDraft = useCallback(
    (text: string) => {
      draftBufRef.current += text;
      if (rafRef.current == null) {
        rafRef.current =
          typeof requestAnimationFrame === "function"
            ? requestAnimationFrame(flushDraft)
            : (setTimeout(flushDraft, 16) as unknown as number);
      }
    },
    [flushDraft],
  );
  const cancelDraftFlush = useCallback(() => {
    if (rafRef.current != null) {
      if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(rafRef.current);
      else clearTimeout(rafRef.current);
      rafRef.current = null;
    }
    draftBufRef.current = "";
  }, []);

  useEffect(() => {
    let disposed = false;
    setConnection("connecting");
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url =
      `${proto}//${window.location.host}/ws/chat` + `?session=${encodeURIComponent(sessionName)}`;
    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      if (!disposed) setConnection("open");
    };

    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      let env: Record<string, unknown>;
      try {
        env = JSON.parse(ev.data);
      } catch {
        return;
      }
      handleEnvelope(env, { dispatch, appendDraft, cancelDraft: cancelDraftFlush });
    };

    const scheduleReconnect = () => {
      if (disposed) return;
      cancelDraftFlush();
      dispatch({ t: "resetTransient" });
      setTimeout(
        () => {
          if (!disposed) setAttempt((n) => n + 1);
        },
        Math.min(8000, 1200 + 400 * attempt),
      );
    };
    ws.onclose = () => {
      if (disposed) return;
      setConnection("closed");
      scheduleReconnect();
    };
    ws.onerror = () => {
      if (disposed) return;
      setConnection("closed");
    };

    return () => {
      disposed = true;
      cancelDraftFlush();
      try {
        ws.close();
      } catch {
        // ignore
      }
      wsRef.current = null;
    };
    // attempt drives reconnects; sessionName remount is handled by the key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionName, attempt]);

  const send = useCallback((text: string, images: OutgoingImage[]) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    ws.send(
      JSON.stringify({
        type: "user_message",
        text,
        images: images.map((i) => ({ mediaType: i.mediaType, data: i.data, thumb: i.thumb })),
      }),
    );
    // Optimistically show the typing indicator without waiting for the
    // first stream_event round-trip.
    dispatch({ t: "generating", on: true });
    dispatch({ t: "error", message: null });
    return true;
  }, []);

  const interrupt = useCallback(() => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: "interrupt" }));
    }
  }, []);

  const setModel = useCallback((model: string) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: "set_model", model }));
    }
  }, []);

  const respondPermission = useCallback(
    (
      requestId: string,
      allow: boolean,
      answers?: Record<string, string | string[]>,
      message?: string,
    ) => {
      const ws = wsRef.current;
      if (!ws || ws.readyState !== WebSocket.OPEN) return;
      ws.send(
        JSON.stringify({
          type: "permission_response",
          request_id: requestId,
          allow,
          answers: answers ?? null,
          message: message ?? null,
        }),
      );
      // Retire the card only after the answer is actually enqueued; the
      // backend also emits `chat_permission_resolved` for other clients.
      dispatch({ t: "permResolve", requestId });
    },
    [],
  );

  const setPermissionMode = useCallback((mode: string | null) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: "set_permission_mode", mode }));
      // Reflect the intent immediately so the toggle feels responsive; the
      // backend's `chat_status` confirms (or corrects) it after the respawn.
      dispatch({ t: "permissionMode", mode });
    }
  }, []);

  const reconnect = useCallback(() => setAttempt((n) => n + 1), []);

  return {
    ...state,
    connection,
    send,
    interrupt,
    setModel,
    respondPermission,
    setPermissionMode,
    reconnect,
  };
}

type Handlers = {
  dispatch: React.Dispatch<Action>;
  appendDraft: (text: string) => void;
  cancelDraft: () => void;
};

function handleEnvelope(env: Record<string, unknown>, h: Handlers) {
  const { dispatch, appendDraft, cancelDraft } = h;
  const type = env.type as string | undefined;
  switch (type) {
    case "user_input": {
      const seq = env._seq as number;
      const blocks = (env.content as UserContentBlock[]) ?? [];
      dispatch({ t: "commit", msg: { seq, role: "user", blocks } });
      break;
    }
    case "assistant": {
      // The committed block carries the full text; drop any buffered deltas
      // so they don't leak into the next block's draft.
      cancelDraft();
      const seq = env._seq as number;
      const message = env.message as { content?: AssistantBlock[] } | undefined;
      const blocks = (message?.content ?? []) as AssistantBlock[];
      dispatch({ t: "commit", msg: { seq, role: "assistant", blocks } });
      break;
    }
    case "user": {
      // Tool-result echo: fold into the tool-result map rather than render
      // it as a standalone message.
      const message = env.message as { content?: unknown[] } | undefined;
      for (const b of message?.content ?? []) {
        if (b && typeof b === "object" && (b as { type?: string }).type === "tool_result") {
          const tr = b as { tool_use_id: string; content?: unknown; is_error?: boolean };
          dispatch({
            t: "toolResult",
            id: tr.tool_use_id,
            result: { content: toolResultText(tr.content), isError: !!tr.is_error },
          });
        }
      }
      break;
    }
    case "system": {
      if (env.subtype === "init" && typeof env.model === "string") {
        dispatch({ t: "model", model: env.model });
      }
      // `init` and `status` both report the live permission mode (plan vs
      // default); track it so the composer toggle reflects reality (#95).
      if (typeof env.permissionMode === "string") {
        dispatch({ t: "permissionMode", mode: env.permissionMode });
      }
      break;
    }
    case "stream_event": {
      const event = env.event as
        | {
            type?: string;
            delta?: Record<string, unknown>;
            content_block?: Record<string, unknown>;
          }
        | undefined;
      if (!event) break;
      if (event.type === "message_start") {
        dispatch({ t: "generating", on: true });
      } else if (event.type === "content_block_start") {
        cancelDraft();
        const cb = event.content_block as { type?: string } | undefined;
        if (cb?.type === "thinking") dispatch({ t: "draftStart", kind: "thinking" });
        else if (cb?.type === "text") dispatch({ t: "draftStart", kind: "text" });
      } else if (event.type === "content_block_delta") {
        const d = event.delta as { type?: string; text?: string; thinking?: string } | undefined;
        if (d?.type === "text_delta" && d.text) appendDraft(d.text);
        else if (d?.type === "thinking_delta" && d.thinking) appendDraft(d.thinking);
      }
      break;
    }
    case "result": {
      cancelDraft();
      dispatch({ t: "generating", on: false });
      break;
    }
    case "chat_permission": {
      const req = parsePermission(env);
      if (req) dispatch({ t: "permAdd", req });
      break;
    }
    case "chat_permission_resolved": {
      const id = env.request_id as string | undefined;
      if (id) dispatch({ t: "permResolve", requestId: id });
      break;
    }
    case "chat_status": {
      const st = env.state as string | undefined;
      if (st === "running") dispatch({ t: "lifecycle", lifecycle: "running" });
      else if (st === "dead") {
        dispatch({ t: "lifecycle", lifecycle: "dead" });
        // A dead process can no longer answer pending requests; drop them so
        // the UI doesn't show un-answerable cards.
        dispatch({ t: "permClear" });
      } else if (st === "switching") dispatch({ t: "lifecycle", lifecycle: "switching" });
      if (typeof env.model === "string") dispatch({ t: "model", model: env.model });
      // A mode switch reports the new permission mode here (the value may be
      // null when leaving plan mode), so reconcile the composer toggle (#95).
      if ("permissionMode" in env) {
        dispatch({
          t: "permissionMode",
          mode: typeof env.permissionMode === "string" ? env.permissionMode : null,
        });
      }
      break;
    }
    case "chat_error": {
      dispatch({ t: "error", message: String(env.message ?? "エラーが発生しました") });
      dispatch({ t: "generating", on: false });
      break;
    }
    default:
      break;
  }
}

import { useState } from "react";
import { ChatMarkdown } from "@/components/chat/ChatMarkdown";
import type { AskQuestion, PermissionRequest } from "@/lib/chat-types";

/**
 * Interactive permission UI (#95): AskUserQuestion and plan approval. The
 * backend forwards the pending tool request; the user's choice is sent back
 * over the socket and the card retires when the request resolves.
 */
export function PermissionView({
  req,
  onRespond,
  disabled,
}: {
  req: PermissionRequest;
  onRespond: (
    requestId: string,
    allow: boolean,
    answers?: Record<string, string | string[]>,
    message?: string,
  ) => void;
  disabled: boolean;
}) {
  if (req.tool === "AskUserQuestion") {
    return <AskQuestionCard req={req} onRespond={onRespond} disabled={disabled} />;
  }
  return <PlanApprovalCard req={req} onRespond={onRespond} disabled={disabled} />;
}

function AskQuestionCard({
  req,
  onRespond,
  disabled,
}: {
  req: Extract<PermissionRequest, { tool: "AskUserQuestion" }>;
  onRespond: (
    requestId: string,
    allow: boolean,
    answers?: Record<string, string | string[]>,
    message?: string,
  ) => void;
  disabled: boolean;
}) {
  // Selected label(s) per question index.
  const [picked, setPicked] = useState<Record<number, string[]>>({});

  const toggle = (qi: number, label: string, multi: boolean) => {
    setPicked((prev) => {
      const cur = prev[qi] ?? [];
      if (multi) {
        const next = cur.includes(label) ? cur.filter((l) => l !== label) : [...cur, label];
        return { ...prev, [qi]: next };
      }
      return { ...prev, [qi]: [label] };
    });
  };

  const allAnswered = req.questions.every((_, qi) => (picked[qi]?.length ?? 0) > 0);

  const submit = () => {
    const answers: Record<string, string | string[]> = {};
    req.questions.forEach((q, qi) => {
      const sel = picked[qi] ?? [];
      answers[q.question] = q.multiSelect ? sel : (sel[0] ?? "");
    });
    onRespond(req.requestId, true, answers);
  };

  return (
    <Card accent>
      <CardHeader icon="?" label="質問" />
      <div className="px-3.5 py-3 space-y-4">
        {req.questions.map((q, qi) => (
          <QuestionBlock
            key={qi}
            q={q}
            selected={picked[qi] ?? []}
            onToggle={(label) => toggle(qi, label, !!q.multiSelect)}
            disabled={disabled}
          />
        ))}
        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={() =>
              onRespond(req.requestId, false, undefined, "ユーザーが回答をスキップしました。")
            }
            disabled={disabled}
            className="h-8 px-3 rounded-lg text-[12.5px] text-fg-subtle hover:text-fg hover:bg-surface-muted disabled:opacity-40 transition-colors"
          >
            スキップ
          </button>
          <button
            type="button"
            onClick={submit}
            disabled={disabled || !allAnswered}
            className="h-8 px-4 rounded-lg bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-[12.5px] font-medium"
          >
            回答する
          </button>
        </div>
      </div>
    </Card>
  );
}

function QuestionBlock({
  q,
  selected,
  onToggle,
  disabled,
}: {
  q: AskQuestion;
  selected: string[];
  onToggle: (label: string) => void;
  disabled: boolean;
}) {
  return (
    <div className="space-y-2">
      <div>
        {q.header && (
          <div className="text-[10px] uppercase tracking-wide text-fg-faint mb-0.5">{q.header}</div>
        )}
        <div className="text-[13.5px] text-fg font-medium">{q.question}</div>
        {q.multiSelect && <div className="text-[11px] text-fg-faint mt-0.5">複数選択できます</div>}
      </div>
      <div className="grid gap-1.5">
        {q.options.map((opt) => {
          const active = selected.includes(opt.label);
          return (
            <button
              key={opt.label}
              type="button"
              onClick={() => onToggle(opt.label)}
              disabled={disabled}
              className={[
                "w-full text-left rounded-lg border px-3 py-2 transition-colors disabled:opacity-40",
                active
                  ? "border-accent bg-accent/10"
                  : "border-line bg-surface hover:border-line-strong hover:bg-surface-muted",
              ].join(" ")}
            >
              <div className="flex items-start gap-2">
                <span
                  className={[
                    "mt-0.5 w-3.5 h-3.5 shrink-0 flex items-center justify-center border",
                    q.multiSelect ? "rounded-[4px]" : "rounded-full",
                    active ? "bg-accent border-accent text-accent-fg" : "border-line-strong",
                  ].join(" ")}
                >
                  {active && (
                    <svg viewBox="0 0 12 12" fill="none" className="w-2.5 h-2.5">
                      <path
                        d="M2.5 6.2l2.2 2.3 4.8-5"
                        stroke="currentColor"
                        strokeWidth="1.8"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  )}
                </span>
                <span className="min-w-0">
                  <span className="block text-[13px] text-fg">{opt.label}</span>
                  {opt.description && (
                    <span className="block text-[11.5px] text-fg-subtle mt-0.5">
                      {opt.description}
                    </span>
                  )}
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PlanApprovalCard({
  req,
  onRespond,
  disabled,
}: {
  req: Extract<PermissionRequest, { tool: "ExitPlanMode" }>;
  onRespond: (
    requestId: string,
    allow: boolean,
    answers?: Record<string, string | string[]>,
    message?: string,
  ) => void;
  disabled: boolean;
}) {
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");

  return (
    <Card accent>
      <CardHeader icon="◆" label="計画の承認" />
      <div className="px-3.5 py-3 space-y-3">
        <div className="rounded-lg border border-line bg-surface-sunken/40 px-3 py-2 max-h-80 overflow-y-auto scroll-thin">
          {req.plan ? (
            <ChatMarkdown text={req.plan} />
          ) : (
            <div className="text-[12.5px] text-fg-subtle italic">（計画の本文がありません）</div>
          )}
        </div>

        {rejecting ? (
          <div className="space-y-2">
            <textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              rows={2}
              autoFocus
              placeholder="却下の理由（任意）— 修正してほしい点など"
              className="w-full resize-none rounded-lg border border-line bg-surface px-3 py-2 text-[13px] text-fg placeholder:text-fg-faint outline-none focus:border-line-strong"
            />
            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setRejecting(false)}
                disabled={disabled}
                className="h-8 px-3 rounded-lg text-[12.5px] text-fg-subtle hover:text-fg hover:bg-surface-muted disabled:opacity-40 transition-colors"
              >
                戻る
              </button>
              <button
                type="button"
                onClick={() =>
                  onRespond(
                    req.requestId,
                    false,
                    undefined,
                    reason.trim() || "ユーザーが計画を却下しました。",
                  )
                }
                disabled={disabled}
                className="h-8 px-4 rounded-lg bg-danger text-danger-fg hover:bg-danger/90 disabled:opacity-40 transition-colors text-[12.5px] font-medium"
              >
                却下して送信
              </button>
            </div>
          </div>
        ) : (
          <div className="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => setRejecting(true)}
              disabled={disabled}
              className="h-8 px-3.5 rounded-lg border border-line-strong text-fg text-[12.5px] hover:bg-surface-muted disabled:opacity-40 transition-colors"
            >
              却下
            </button>
            <button
              type="button"
              onClick={() => onRespond(req.requestId, true)}
              disabled={disabled}
              className="h-8 px-4 rounded-lg bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-40 transition-colors text-[12.5px] font-medium"
            >
              承認して実行
            </button>
          </div>
        )}
      </div>
    </Card>
  );
}

function Card({ accent, children }: { accent?: boolean; children: React.ReactNode }) {
  return (
    <div
      className={[
        "rounded-xl border overflow-hidden shadow-sm",
        accent ? "border-accent/40 bg-surface-elev" : "border-line bg-surface-elev",
      ].join(" ")}
    >
      {children}
    </div>
  );
}

function CardHeader({ icon, label }: { icon: string; label: string }) {
  return (
    <div className="flex items-center gap-2 px-3.5 h-9 border-b border-line bg-accent/10">
      <span className="inline-flex items-center justify-center w-5 h-5 rounded-md bg-accent/15 text-accent text-[12px] font-semibold">
        {icon}
      </span>
      <span className="text-[12.5px] font-medium text-fg">{label}</span>
      <span className="ml-auto text-[10.5px] text-fg-faint">あなたの操作が必要です</span>
    </div>
  );
}

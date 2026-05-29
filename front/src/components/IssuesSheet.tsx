import { useEffect, useState } from "react";
import useSWR from "swr";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Sheet, SheetBody, SheetFooter, SheetHeader } from "./Sheet";
import { IconChevronRight, IconSearch } from "./icons";
import { Badge, Button, Input, Spinner } from "@/components/ui";

/** issues fetched per page; "load more" requests one more page worth. */
const PAGE = 30;

type IssueSummary = {
  number: number;
  title: string;
  state: string;
  labels: string[];
  updatedAt: string;
  author: string;
};
type IssueDetail = {
  number: number;
  title: string;
  body: string;
  state: string;
  labels: string[];
  url: string;
  author: string;
};

const fetcher = async (url: string) => {
  const r = await fetch(url);
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
};

/** Compose the initial prompt handed to the agent when launching from an issue. */
export function buildIssuePrompt(issue: IssueDetail): string {
  const body = issue.body.trim() || "(本文なし)";
  return [
    `GitHub issue #${issue.number}「${issue.title}」に取り組んでください。`,
    "",
    body,
    "",
    `参照: ${issue.url}`,
  ].join("\n");
}

type Props = {
  isOpen: boolean;
  projectName: string;
  projectPath: string;
  onClose: () => void;
  onLaunchIssue: (prompt: string, number: number, title: string) => void;
};

export function IssuesSheet({ isOpen, projectName, projectPath, onClose, onLaunchIssue }: Props) {
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [limit, setLimit] = useState(PAGE);

  // Reset all list state whenever the sheet (re)opens.
  useEffect(() => {
    if (!isOpen) {
      setSelected(null);
      setQuery("");
      setDebounced("");
      setLimit(PAGE);
    }
  }, [isOpen]);

  // Debounce the search box, and reset paging back to the first page on
  // every new query so "load more" counts from the top of the new result.
  useEffect(() => {
    const t = setTimeout(() => {
      setDebounced(query.trim());
      setLimit(PAGE);
    }, 300);
    return () => clearTimeout(t);
  }, [query]);

  const listKey =
    isOpen && selected === null
      ? `/api/projects/issues?path=${encodeURIComponent(projectPath)}&limit=${limit}` +
        (debounced ? `&search=${encodeURIComponent(debounced)}` : "")
      : null;
  const {
    data: listData,
    error: listError,
    isLoading: listLoading,
    isValidating: listValidating,
  } = useSWR<{ issues: IssueSummary[] }>(listKey, fetcher, { keepPreviousData: true });

  const detailKey =
    isOpen && selected !== null
      ? `/api/projects/issue?path=${encodeURIComponent(projectPath)}&number=${selected}`
      : null;
  const {
    data: detailData,
    error: detailError,
    isLoading: detailLoading,
  } = useSWR<{ issue: IssueDetail }>(detailKey, fetcher);

  const issues = listData?.issues ?? [];
  const detail = detailData?.issue ?? null;
  // gh returns a flat, limit-capped list with no cursor — if we got a full
  // page back there may be more, so offer to widen the limit.
  const canLoadMore = issues.length >= limit;

  return (
    <Sheet open={isOpen} onClose={onClose} maxWidth="2xl" tall>
      <SheetHeader
        title={selected === null ? `${projectName} の issue` : `#${selected}`}
        subtitle={selected === null ? projectPath : undefined}
        onClose={onClose}
      />
      <SheetBody>
        {selected === null ? (
          <div className="space-y-3">
            <Input
              type="search"
              placeholder="issue を検索 (例: bug in:title)"
              value={query}
              onValueChange={setQuery}
              clearable
              leftSlot={<IconSearch className="w-4 h-4" />}
            />
            {listLoading ? (
              <div className="flex justify-center py-10">
                <Spinner size="md" />
              </div>
            ) : listError ? (
              <ErrorBox message={(listError as Error).message} />
            ) : issues.length === 0 ? (
              <div className="py-10 text-center text-sm text-fg-subtle">
                {debounced ? "一致する issue がありません" : "open な issue がありません"}
              </div>
            ) : (
              <>
                <ul className="space-y-1">
                  {issues.map((it) => (
                    <li key={it.number}>
                      <button
                        type="button"
                        onClick={() => setSelected(it.number)}
                        className="w-full text-left flex items-center gap-2 px-3 py-2.5 rounded-md border border-line bg-surface hover:bg-surface-muted transition-colors"
                      >
                        <span className="text-2xs font-mono text-fg-faint shrink-0 tabular-nums">
                          #{it.number}
                        </span>
                        <span className="flex-1 min-w-0 text-sm text-fg truncate">{it.title}</span>
                        {it.labels.slice(0, 2).map((l) => (
                          <Badge key={l} tone="violet" className="shrink-0">
                            {l}
                          </Badge>
                        ))}
                        <IconChevronRight className="w-3.5 h-3.5 text-fg-faint shrink-0" />
                      </button>
                    </li>
                  ))}
                </ul>
                {canLoadMore && (
                  <Button
                    variant="secondary"
                    size="md"
                    className="w-full"
                    loading={listValidating}
                    onClick={() => setLimit((l) => l + PAGE)}
                  >
                    もっと読み込む
                  </Button>
                )}
              </>
            )}
          </div>
        ) : detailLoading ? (
          <div className="flex justify-center py-10">
            <Spinner size="md" />
          </div>
        ) : detailError ? (
          <ErrorBox message={(detailError as Error).message} />
        ) : detail ? (
          <div>
            <button
              type="button"
              onClick={() => setSelected(null)}
              className="text-xs text-fg-subtle hover:text-fg inline-flex items-center gap-1 mb-3"
            >
              ‹ 一覧へ戻る
            </button>
            <div className="flex items-start gap-2 flex-wrap mb-1">
              <h2 className="text-base font-semibold text-fg flex-1 min-w-0">{detail.title}</h2>
              <Badge tone={detail.state === "OPEN" ? "emerald" : "violet"}>{detail.state}</Badge>
            </div>
            <div className="text-2xs text-fg-faint mb-4">
              #{detail.number} · {detail.author}
              {detail.labels.length > 0 && (
                <span className="ml-2 inline-flex gap-1">
                  {detail.labels.map((l) => (
                    <Badge key={l} tone="violet">
                      {l}
                    </Badge>
                  ))}
                </span>
              )}
            </div>
            {detail.body.trim() ? (
              <div className="md-preview text-sm">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{detail.body}</ReactMarkdown>
              </div>
            ) : (
              <div className="text-sm text-fg-subtle">(本文なし)</div>
            )}
          </div>
        ) : null}
      </SheetBody>
      {selected !== null && detail && (
        <SheetFooter>
          <Button
            variant="secondary"
            size="lg"
            onClick={() => setSelected(null)}
            className="flex-1"
          >
            戻る
          </Button>
          <Button
            variant="primary"
            size="lg"
            className="flex-1"
            onClick={() => onLaunchIssue(buildIssuePrompt(detail), detail.number, detail.title)}
          >
            この issue で起動
          </Button>
        </SheetFooter>
      )}
    </Sheet>
  );
}

function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-md border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger">
      <div className="font-medium mb-1">issue を取得できませんでした</div>
      <div className="text-xs font-mono break-all opacity-80">{message}</div>
      <div className="text-xs text-fg-subtle mt-2">
        GitHub リモートが設定され、`gh` が認証済みである必要があります。
      </div>
    </div>
  );
}

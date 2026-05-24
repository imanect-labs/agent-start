import useSWR from "swr";
import { Spinner } from "@/components/ui";
import { DiffView } from "./DiffView";
import type { DiffMode } from "./tab-types";

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    const json = await r.json();
    if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
    return json;
  });

type Props = {
  cwd: string;
  file: string;
  mode: DiffMode;
};

export function DiffTabView({ cwd, file, mode }: Props) {
  const url = `/api/git/diff?path=${encodeURIComponent(cwd)}&file=${encodeURIComponent(file)}&mode=${mode}`;
  const { data, error, isLoading } = useSWR<{
    diff: string;
    truncated: boolean;
    isUntracked: boolean;
  }>(url, fetcher);

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <div className="flex items-center gap-3 px-4 h-9 border-b border-line bg-surface shrink-0">
        <div className="flex-1 min-w-0 text-[12px] font-mono truncate">{file}</div>
        <span className="text-[10px] uppercase tracking-wider text-fg-faint">{mode}</span>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin p-4">
        {isLoading ? (
          <div className="flex justify-center py-12">
            <Spinner size="md" />
          </div>
        ) : error ? (
          <div className="text-center text-xs text-danger py-8">
            取得失敗: {(error as Error).message}
          </div>
        ) : (
          <>
            <DiffView text={data?.diff ?? ""} />
            {data?.truncated && (
              <div className="text-center text-xs text-warn py-3">
                (diff が大きいため一部省略されました)
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

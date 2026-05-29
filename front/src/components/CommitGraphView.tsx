import useSWR from "swr";
import { useMemo } from "react";
import { Skeleton, SkeletonRows } from "@/components/ui";

type CommitNode = {
  sha: string;
  shortSha: string;
  parents: string[];
  subject: string;
  authorName: string;
  authorEmail: string;
  authoredAt: number;
  refs: string[];
};

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    const json = await r.json();
    if (!r.ok) throw new Error(json.error ?? `HTTP ${r.status}`);
    return json;
  });

// Lane colors come from the harmonized --lane-N tokens (theme-aware) rather
// than raw hex, so the graph matches the rest of the palette in both themes.
const LANE_COLORS = [
  "rgb(var(--lane-1))",
  "rgb(var(--lane-2))",
  "rgb(var(--lane-3))",
  "rgb(var(--lane-4))",
  "rgb(var(--lane-5))",
  "rgb(var(--lane-6))",
  "rgb(var(--lane-7))",
  "rgb(var(--lane-8))",
];

const ROW_H = 44;
const COL_W = 14;
const DOT_X = 12;

type Placed = {
  c: CommitNode;
  lane: number;
  /** Lanes that have a line passing through this row (lane -> child sha). */
  links: { fromLane: number; toLane: number }[];
};

/**
 * Assign each commit a lane (column) from its parent edges — a compact
 * first-fit allocator that mirrors how `git log --graph` lays out rails.
 */
function layout(commits: CommitNode[]): { placed: Placed[]; maxLane: number } {
  // lane -> sha the lane is currently "waiting" to reach.
  const lanes: (string | null)[] = [];
  const placed: Placed[] = [];
  let maxLane = 0;

  const claim = (sha: string) => {
    const existing = lanes.findIndex((l) => l === sha);
    if (existing >= 0) return existing;
    const free = lanes.findIndex((l) => l === null);
    if (free >= 0) {
      lanes[free] = sha;
      return free;
    }
    lanes.push(sha);
    return lanes.length - 1;
  };

  for (const c of commits) {
    const lane = claim(c.sha);
    lanes[lane] = null; // this commit is now resolved on its lane

    const links: { fromLane: number; toLane: number }[] = [];
    // First parent continues this lane; extra parents take new lanes.
    c.parents.forEach((p, i) => {
      const toLane = i === 0 ? ((lanes[lane] = p), lane) : claim(p);
      links.push({ fromLane: lane, toLane });
    });

    maxLane = Math.max(maxLane, lane, lanes.length - 1);
    placed.push({ c, lane, links });
  }
  return { placed, maxLane };
}

function relTime(unixSec: number): string {
  const diff = Date.now() / 1000 - unixSec;
  const d = Math.floor(diff / 86400);
  if (d > 0) return `${d}d ago`;
  const h = Math.floor(diff / 3600);
  if (h > 0) return `${h}h ago`;
  const m = Math.floor(diff / 60);
  if (m > 0) return `${m}m ago`;
  return "just now";
}

export function CommitGraphView({ cwd }: { cwd: string }) {
  const key = cwd ? `/api/git/log?path=${encodeURIComponent(cwd)}&limit=200` : null;
  const { data, error, isLoading } = useSWR<{ commits: CommitNode[] }>(key, fetcher);

  const { placed, maxLane } = useMemo(() => layout(data?.commits ?? []), [data]);
  const graphW = (maxLane + 1) * COL_W + DOT_X;

  if (isLoading && !data) {
    return (
      <div className="p-4 space-y-3">
        <Skeleton style={{ height: 16, width: "40%" }} />
        <SkeletonRows n={8} rowHeight={ROW_H} className="mt-2" />
      </div>
    );
  }
  if (error) {
    return <Empty>取得に失敗: {(error as Error).message}</Empty>;
  }
  if (placed.length === 0) {
    return <Empty>コミットがありません</Empty>;
  }

  return (
    <div className="flex-1 min-h-0 overflow-auto scroll-thin p-2">
      <div className="relative" style={{ minHeight: placed.length * ROW_H }}>
        {/* SVG rails behind the rows. */}
        <svg
          className="absolute top-0 left-0"
          width={graphW}
          height={placed.length * ROW_H}
          style={{ pointerEvents: "none" }}
        >
          {placed.map((row, i) =>
            row.links.map((lnk, j) => {
              const y1 = i * ROW_H + ROW_H / 2;
              const y2 = (i + 1) * ROW_H + ROW_H / 2;
              const x1 = DOT_X + lnk.fromLane * COL_W;
              const x2 = DOT_X + lnk.toLane * COL_W;
              return (
                <path
                  key={`${i}-${j}`}
                  d={`M ${x1} ${y1} C ${x1} ${(y1 + y2) / 2}, ${x2} ${(y1 + y2) / 2}, ${x2} ${y2}`}
                  fill="none"
                  stroke={LANE_COLORS[lnk.toLane % LANE_COLORS.length]}
                  strokeWidth={1.5}
                />
              );
            }),
          )}
          {placed.map((row, i) => {
            const cx = DOT_X + row.lane * COL_W;
            const cy = i * ROW_H + ROW_H / 2;
            return (
              <circle
                key={row.c.sha}
                cx={cx}
                cy={cy}
                r={4}
                fill={LANE_COLORS[row.lane % LANE_COLORS.length]}
                stroke="rgb(var(--app))"
                strokeWidth={1.5}
              />
            );
          })}
        </svg>

        {/* Commit rows. */}
        <ul style={{ marginLeft: graphW }}>
          {placed.map((row) => (
            <li
              key={row.c.sha}
              className="flex flex-col justify-center px-2 border-b border-line/50"
              style={{ height: ROW_H }}
            >
              <div className="flex items-center gap-2 min-w-0">
                {row.c.refs.map((r) => (
                  <span
                    key={r}
                    className="shrink-0 text-2xs font-mono px-1.5 py-0.5 rounded-full bg-accent/10 text-accent border border-accent/20"
                  >
                    {r}
                  </span>
                ))}
                <span className="text-xs truncate flex-1 min-w-0">{row.c.subject}</span>
                <span className="shrink-0 font-mono text-2xs text-fg-faint">{row.c.shortSha}</span>
              </div>
              <div className="text-2xs text-fg-faint truncate">
                {row.c.authorName} · {relTime(row.c.authoredAt)}
              </div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex-1 flex items-center justify-center text-center text-xs text-fg-subtle p-6">
      {children}
    </div>
  );
}

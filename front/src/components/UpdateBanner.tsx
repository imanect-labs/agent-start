import { useEffect, useState } from "react";

type UpdateCheck = {
  current: string;
  latest: string | null;
  available: boolean;
  htmlUrl?: string;
};

const dismissKey = (latest: string) => `agent-start:update-dismissed:${latest}`;

/**
 * Polls `/v1/update-check` once on mount and, when a newer release exists,
 * shows a thin dismissible banner linking to the release. The dismissal is
 * remembered per-version in localStorage so it doesn't nag after the user
 * acknowledges a given release.
 */
export function UpdateBanner() {
  const [info, setInfo] = useState<UpdateCheck | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const res = await fetch("/v1/update-check");
        if (!res.ok) return;
        const data: UpdateCheck = await res.json();
        if (cancelled || !data.available || !data.latest) return;
        if (localStorage.getItem(dismissKey(data.latest))) return;
        setInfo(data);
      } catch {
        // best-effort; stay silent on failure
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (!info || !info.latest || dismissed) return null;

  return (
    <div
      role="status"
      className="fixed inset-x-0 top-0 z-[150] flex items-center justify-center gap-2 border-b border-line bg-surface-elev px-3 py-1.5 text-xs text-fg safe-top"
    >
      <span>
        Update available: <span className="font-medium">{info.latest}</span>{" "}
        <span className="text-fg-subtle">(current {info.current})</span>
      </span>
      {info.htmlUrl && (
        <a
          href={info.htmlUrl}
          target="_blank"
          rel="noreferrer"
          className="font-medium underline underline-offset-2 hover:text-fg"
        >
          View release
        </a>
      )}
      <button
        type="button"
        aria-label="Dismiss update notification"
        className="ml-1 rounded px-1.5 text-fg-subtle hover:bg-surface hover:text-fg"
        onClick={() => {
          if (info.latest) localStorage.setItem(dismissKey(info.latest), "1");
          setDismissed(true);
        }}
      >
        ✕
      </button>
    </div>
  );
}

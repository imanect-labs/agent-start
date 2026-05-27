import { useEffect, useState } from "react";

type Props = {
  sessionName: string;
};

type OpenResponse = {
  url: string;
  ws_port: number;
  display: number;
};

type State =
  | { status: "loading" }
  | { status: "ready"; url: string; display: number }
  | { status: "error"; message: string; dependency: boolean };

export function GuiView({ sessionName }: Props) {
  const [state, setState] = useState<State>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(sessionName)}/novnc`, {
          method: "POST",
        });
        if (!res.ok) {
          const body = (await res.json().catch(() => ({}))) as { error?: string };
          if (!cancelled) {
            setState({
              status: "error",
              message: body?.error || `HTTP ${res.status}`,
              dependency: res.status === 424,
            });
          }
          return;
        }
        const data = (await res.json()) as OpenResponse;
        if (!cancelled) {
          setState({ status: "ready", url: data.url, display: data.display });
        }
      } catch (e) {
        if (!cancelled) {
          setState({
            status: "error",
            message: e instanceof Error ? e.message : String(e),
            dependency: false,
          });
        }
      }
    })();
    return () => {
      cancelled = true;
      // Best-effort teardown so noVNC processes don't pile up when the
      // user closes the tab. Fire-and-forget — failures are non-fatal.
      void fetch(`/api/sessions/${encodeURIComponent(sessionName)}/novnc`, {
        method: "DELETE",
      });
    };
  }, [sessionName]);

  if (state.status === "loading") {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center text-fg-subtle text-sm">
        noVNC バックエンドを起動中…
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto p-6 text-sm">
        <div className="max-w-2xl mx-auto">
          <h3 className="text-base font-semibold text-fg">GUI を表示できません</h3>
          <p className="mt-2 text-fg-muted">{state.message}</p>
          {state.dependency && (
            <div className="mt-4 rounded-md border border-line bg-surface-muted p-4 text-fg-muted">
              <p className="font-medium text-fg">必要な依存をインストールしてください</p>
              <pre className="mt-2 text-xs whitespace-pre-wrap">
                {`# Debian/Ubuntu
sudo apt install tigervnc-standalone-server novnc websockify

# macOS
brew install tiger-vnc
pipx install websockify
git clone --depth=1 https://github.com/novnc/noVNC.git ~/.local/share/novnc
export AGENT_START_NOVNC_DIR=~/.local/share/novnc`}
              </pre>
              <p className="mt-3 text-xs">
                インストール後、host を再起動してこのタブをもう一度開いてください。
              </p>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 min-h-0 flex flex-col bg-app">
      <div className="px-3 py-1.5 text-[11px] text-fg-faint border-b border-line">
        <code>DISPLAY=:{state.display}</code> でセッション内から GUI アプリを起動できます
      </div>
      <iframe
        title="noVNC"
        src={state.url}
        className="flex-1 min-h-0 w-full border-0 bg-black"
        // noVNC needs same-origin so the keyboard/clipboard helpers work;
        // we proxy under /vnc/<name>/ so no extra sandbox flags are needed.
      />
    </div>
  );
}

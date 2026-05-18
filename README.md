# agent-start

スマホから tailnet 経由で自宅 Ubuntu サーバ上の **claude code / codex CLI** セッションを起動・管理する Web ランチャー。

- リポジトリ一覧 → タップ → PTY セッション内で `claude` か `codex` を起動
- 起動済みセッションの一覧、プレビュー、停止
- 起動時に **git worktree を作成して起動** をチェック可能
- セッション停止時に紐付く worktree (と branch) も連動削除するか選択可能
- 起動フラグ (`--dangerously-skip-permissions` / `--full-auto` 等) を UI で設定・永続化
- **ブラウザを閉じてもセッションは生き続ける** (Rust ホストプロセスが PTY を保持)

## アーキテクチャ

```
ブラウザ ── tailnet ── Next.js (フロント, :3000)
                          │  rewrites /api/*, /v1/*, /ws/*
                          ▼
                     agent-start-host (Rust, :3030)
                          │  axum + tokio + portable-pty + sqlx
                          ▼
                     bash -lc 'claude ...' / 'codex --full-auto' / ...
```

`agent-start-host` は単一バイナリで、HTTP/WebSocket・PTY 多重化・SQLite 永続化を担う。
tmux への外部依存はない。

## セットアップ

ホスト (Rust バックエンド) とフロント (Next.js SPA) は別プロセスで動く。

```bash
git clone <repo>
cd agent-start
npm install

# 1. Rust ホストをビルドして起動 (デフォルト 127.0.0.1:3030)
(cd server-rs && cargo build --release)
./server-rs/target/release/agent-start-host --bind 0.0.0.0 --port 3030 &

# 2. Next.js フロントを起動 (デフォルト :3000)
npm run start    # or `npm run dev` for hot reload
```

スマホからは `http://<server>:3000` を tailnet 経由で開く。
フロントの `/api/*` `/v1/*` `/ws/*` は自動的に Rust ホストに rewrite される。
ホスト URL を差し替えたい場合は `AGENT_START_HOST_URL=http://other-host:3030` を Next.js に渡す。

設定ファイルとデータは **`~/.agent-start/`** 直下に集約されます (旧 XDG パスに残っている
ファイルは初回起動時に自動移動)。`AGENT_START_HOME` で root を上書き可:

```
~/.agent-start/
├── config.json                  # CLI プリセット (初回起動時に生成)
├── preferences.json             # UI で保存される起動フラグ
├── host.db                      # sessions + pty_history (SQLite)
├── runtime/manifest.json        # 起動中ホストの URL / PID
└── worktrees/<session>/         # git worktree (AGENT_START_WORKTREE_ROOT で上書き可)
```

## CLI

```bash
cargo install --path server-rs/bin/agent-start
agent-start status                # ホストの version
agent-start projects              # ホスト経由でプロジェクト一覧
agent-start sessions              # ホスト経由でセッション一覧
agent-start start --port 3030     # フォアグラウンドでホストを spawn
agent-start stop                  # manifest.json 経由で SIGTERM
```

リモート tailnet 越しに使う場合は `--url http://server:3030` を渡す。

## 設定ファイル

`~/.config/agent-start/config.json`:

```json
{
  "roots": ["/home/shuya/dev"],
  "sessionPrefix": "cc-",
  "shell": "/bin/bash",
  "showHidden": false,
  "gitOnly": false,
  "defaultCli": "claude",
  "clis": {
    "claude": {
      "command": "claude",
      "skipPermissionsFlag": "--dangerously-skip-permissions",
      "label": "Claude Code"
    },
    "codex": {
      "command": "codex",
      "skipPermissionsFlag": "--full-auto",
      "label": "Codex CLI"
    }
  }
}
```

CLI を追加したい場合 (例: aider, opencode) は `clis` にエントリを増やすだけ。
旧 `claudeCommand` キーは自動マイグレーションされる。

## worktree モード

起動シートの「git worktree を作って起動」をチェックすると:

1. `git -C <project> worktree add -b agent-start/<session> ~/.cache/agent-start/worktrees/<session> HEAD` を実行
2. その worktree を cwd として PTY セッション起動

セッション削除時に「worktree も削除」をチェックすると:
- `git worktree remove --force` でツリー削除
- `agent-start/*` ブランチも削除

## ホストを SSH 切断後も動かす

systemd-user で常駐させる例:

```ini
# ~/.config/systemd/user/agent-start.service
[Unit]
Description=agent-start host

[Service]
ExecStart=%h/.cargo/bin/agent-start-host --bind 0.0.0.0 --port 3030
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now agent-start
```

ホストが落ちても、起動済みの `cc-*` セッションは PID + 履歴が SQLite に残るので、復旧時に UI から再接続できる
(child process は ppid=1 に reparent され、systemd-user 起動なら回収可能)。

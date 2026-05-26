# agent-start

> English: [README.md](./README.md)

スマホから tailnet 経由で自宅 Ubuntu サーバ上の **claude code / codex CLI** セッションを起動・管理する Web ランチャー。

- リポジトリ一覧 → タップ → PTY セッション内で `claude` か `codex` を起動
- 起動済みセッションの一覧、プレビュー、停止
- 起動時に **git worktree を作成して起動** をチェック可能
- セッション停止時に紐付く worktree (と branch) も連動削除するか選択可能
- 起動フラグ (`--dangerously-skip-permissions` / `--full-auto` 等) を UI で設定・永続化
- **ブラウザを閉じてもセッションは生き続ける** (Rust ホストプロセスが PTY を保持)

## アーキテクチャ

```
ブラウザ ── tailnet ── agent-start-host (Rust, :3030)
                          │  /api/*, /v1/*, /ws/*  + 静的 SPA (/front/dist/)
                          │  axum + tokio + portable-pty + sqlx
                          ▼
                     bash -lc 'claude ...' / 'codex --full-auto' / ...
```

`agent-start-host` は単一バイナリで、HTTP/WebSocket・PTY 多重化・SQLite 永続化に加え、
ビルド済みの `front/dist/` を `tower-http::ServeDir` で配信する。

フロントは `/front/` 配下の Vite+ + React + TanStack Router SPA。dev 時のみ
`vp dev` (:5173) を別プロセスで立て、Vite+ のプロキシ経由で `:3030` の host を叩く。
本番では host バイナリ単体で完結する。

## インストール

[Releases](https://github.com/imanect-labs/agent-start/releases) ページで Linux (x86_64 / aarch64)、macOS (Apple Silicon / Intel)、Windows (x86_64) 向けのビルド済みバイナリを配布しています。Web UI は `rust-embed` でバイナリに同梱されているため、追加ファイルは不要です。

### Linux / macOS (ワンライナー)

```bash
curl -fsSL https://agentstart.imanect.app/install.sh | bash
```

最新リリースを自動検出し、OS / arch に合うバイナリを `~/.local/bin/agent-start-host` に配置します。環境変数:

- `AGENT_START_VERSION=v0.1.0` — 特定バージョンを固定インストール
- `INSTALL_DIR=/usr/local/bin` — 配置先を変更 (必要に応じて `sudo`)

インストール後:

```bash
agent-start-host --port 3030      # 既定で 127.0.0.1 にバインド
# ブラウザで http://localhost:3030
```

### 手動ダウンロード

[Releases](https://github.com/imanect-labs/agent-start/releases) から該当プラットフォームのアーカイブをダウンロードして展開し、`agent-start-host` を `PATH` 上に配置してください。

| プラットフォーム | アセット名 |
| --- | --- |
| Linux x86_64    | `agent-start-<tag>-x86_64-unknown-linux-gnu.tar.gz`     |
| Linux aarch64   | `agent-start-<tag>-aarch64-unknown-linux-gnu.tar.gz`    |
| macOS arm64     | `agent-start-<tag>-aarch64-apple-darwin.tar.gz`         |
| macOS x86_64    | `agent-start-<tag>-x86_64-apple-darwin.tar.gz`          |
| Windows x86_64  | `agent-start-<tag>-x86_64-pc-windows-msvc.zip`          |

### ソースからビルド

```bash
git clone https://github.com/imanect-labs/agent-start
cd agent-start
(cd front && vp install && vp build)              # SPA を front/dist にビルド
(cd server-rs && cargo build --release)           # rust-embed が front/dist を取り込む
./server-rs/target/release/agent-start-host --port 3030
```

フロントのビルドには `vp` (Vite+) CLI が必要です ([ツールチェイン](#ツールチェイン) 参照)。Rust の最低要件は `server-rs/Cargo.toml` の `rust-version`。

> ⚠️ `127.0.0.1` 以外にバインドする前に [SECURITY.md](./SECURITY.md) を必ず読んでください。本サーバは認証機構を持ちません。

## セットアップ (開発)

開発時は host (Rust) と front (Vite+) を別プロセスで動かす。本番では host が
SPA を配信するので 1 バイナリで完結する。

### ツールチェイン

`vp` (Vite+ CLI) を一度だけインストール:

```bash
# macOS / Linux
curl -fsSL https://vite.plus | bash

# Windows (PowerShell)
# irm https://vite.plus/ps1 | iex

vp env    # インストール確認
```

Vite+ は vite / vitest / oxlint / oxfmt / tsgo / node ランタイムをまとめて管理する
ので、フロント側に追加で `node` / `npm` を入れる必要はない。

### 開発モード (二つのターミナルで)

```bash
git clone <repo>
cd agent-start

# Terminal A: Rust host (:3030)
npm run dev:host
# = cd server-rs && cargo run -p agent-start-host -- --port 3030

# Terminal B: Vite+ SPA (:5173, hot reload)
(cd front && vp install)   # 初回のみ
npm run dev:front
# = cd front && vp dev
```

ブラウザで http://localhost:5173 を開く。Vite+ の `proxy` 設定で
`/api/*` `/v1/*` `/ws/*` は自動で `:3030` の host に転送される。

### 本番モード (single binary)

```bash
# 1. フロントをビルド (host ビルド時に rust-embed が dist を取り込む)
(cd front && vp build)              # -> front/dist/

# 2. host をビルド (front/dist は埋め込まれる)
(cd server-rs && cargo build --release)

# 3. 単一バイナリで起動 (front 配信込み)
./server-rs/target/release/agent-start-host --port 3030
```

> ⚠️ **セキュリティ**: 既定は `127.0.0.1` バインドです。`--bind 0.0.0.0` で外部公開する場合は
> tailnet / VPN / 信頼できる LAN + ファイアウォール配下でのみ利用してください。本サーバには
> 認証機構がなく任意コマンド実行が可能な設計のため、公開インターネットに直接さらしてはいけません。
> 詳細は [SECURITY.md](./SECURITY.md) を参照。

リリースビルドは host バイナリ単体に SPA が同梱されるため、別途 `front/dist` を配布する必要は
ありません。staging で別ビルドの SPA を差し替えたい場合のみ `--frontend-dist <path>` (または
`AGENT_START_FRONTEND_DIST`) で上書きできます。

設定ファイルとデータは **`~/.agent-start/`** 直下に集約されます (旧 XDG パスに残っている
ファイルは初回起動時に自動移動)。`AGENT_START_HOME` で root を上書き可:

```
~/.agent-start/
├── config.json                  # CLI プリセット (初回起動時に生成)
├── preferences.json             # UI で保存される起動フラグ
├── host.db                      # sessions + pty_history (SQLite)
├── runtime/manifest.json        # 起動中ホストの URL / PID
├── projects/                    # クローン/インポートされたプロジェクト群 (AGENT_START_PROJECTS で上書き可)
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

## 設定 (UI)

設定は左サイドバー右上の歯車アイコン →  `/settings` ページから編集できる。
全項目が `~/.agent-start/config.json` と `preferences.json` に保存され、未保存変更がある状態で離脱しようとすると警告が出る。

主な項目:
- **プロジェクトディレクトリ** (旧 `roots`): プロジェクトを探す検索先。デフォルトは `~/.agent-start/projects`。1 行 1 パスで複数指定可。
- **起動デフォルト**: 既定の CLI、権限スキップ、worktree 作成、追加フラグ
- **セッション**: セッション接頭辞、シェル、隠しディレクトリ表示、git のみ

サイドバー左下の **「プロジェクトを追加」** から、Git リポジトリのクローン または ローカルディレクトリのインポート ができる (どちらも `~/.agent-start/projects/` 配下に作成される)。クローン/コピー中はサイドバーのその行にスピナーが出る。プロジェクト行を右クリックすると削除できる (確認モーダルあり)。

## 設定ファイル

`~/.agent-start/config.json` の中身:

```json
{
  "roots": ["/Users/me/.agent-start/projects"],
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

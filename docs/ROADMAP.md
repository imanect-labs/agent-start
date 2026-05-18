# agent-start ロードマップ

## ゴール

agent-start を **Web で動く、セルフホスト可能な superset 代替** に作り直す。

superset.sh は macOS Desktop アプリ。agent-start はそれを **Linux サーバ上で常駐させ、tailnet 経由でブラウザ (PC / スマホ) から使う** ことを前提にした OSS Web 実装を目指す。superset の機能セット (workspace = worktree、agent 並列実行、永続ターミナル、ポート自動検出、diff viewer 等) を Web UI で享受できれば良い。

設計上の判断:

- **superset.sh とのワイヤ互換 (CLI / HTTP API / `~/.superset/` ファイルレイアウト) は追わない**。superset のソースは Elastic License 2.0 で借用に制約があり、また superset SDK / MCP / `superset` CLI スクリプトとの互換性を維持するメリットも薄い。
- **Rust バックエンド**: 速度・メモリ安全性・小さな静的バイナリでの配布を理由に Node/Next.js の API・サーバ部を置換する。CLI とサーバは **別バイナリ** (`agent-start` と `agent-start-host`) として分離 (理由は §2.0)。
- **tmux 撤廃**: PTY 多重化と「切断後も生存」を Rust ホストプロセス内で完結 (`portable-pty` + 自前永続化)。tmux 外部依存をなくす。
- **/front 分離**: 既存 Next.js を捨て、`/front` に Vite + React の薄い SPA を新規構築。バックエンドとは HTTP/WebSocket だけで会話する。
- **モバイル/タブレットでの利用品質**を superset Desktop と差別化のキモにする (superset 自体は macOS Desktop のみ)。

## 1. 機能スコープ

superset.sh (docs.superset.sh) の機能セットを参考にしつつ、Web セルフホスト前提で取捨選択する。`★` = v1 必須 / `☆` = v2 / `−` = 非スコープ。

### 1.1 提供形態

| superset の形態 | agent-start での扱い |
| --- | --- |
| Desktop IDE (macOS Electron) | − 採らない。 |
| Host Server (`superset start --daemon`、127.0.0.1) | ★ Rust 常駐プロセス。既定は `127.0.0.1`、tailnet/LAN 公開は `--bind` 明示。 |
| CLI (`superset` バイナリ) | ★ Rust 製 `agent-start` CLI。サブコマンドは superset を模さず、本ツール独自の語彙で揃える。 |
| Superset Relay (リモートワークスペース横断) | − tailnet で代替。 |
| MCP server デプロイ | ☆ v2 で検討。 |

### 1.2 機能

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| Workspace (= 1 git branch = 1 git worktree) | ★ | 既存の worktree 機能を「1 workspace = 1 branch」モデルに揃える。 |
| Per-workspace terminal (multi-tab + persistence) | ★ | tmux を撤廃し、自前 PTY マネージャで「切断後も生存・出力履歴・スクロールバック」を維持。 |
| Agent integration (Claude Code / Codex / Cursor / Gemini / OpenCode / Pi / Amp など) | ★ | 既存 `clis` 設定を preset モデルに拡張。 |
| Diff Viewer (split / unified / hunk staging) | ★ | 既存 `app/api/git/diff` を強化。staging API を追加。 |
| Ports auto-detection + 終了操作 + ラベル設定 | ★ | host server で listening port を監視 (`/proc/net/tcp` + `/proc/<pid>/`)。 |
| In-app browser pane (ローカル port のプレビュー) | ☆ | iframe で簡易プレビュー。CSP/X-Frame 制約は注記。 |
| Setup / Teardown / Run scripts (リポ内設定 + ユーザ override + local override) | ★ | **本ツール独自のパス** (`.agent-start/config.json` 等)。superset の `.superset/` は読まない。 |
| Automations (RRULE スケジュール、run 履歴) | ☆ | v2。cron-like。 |
| VSCode Web UI 統合 (code-server 同梱 + リバースプロキシ) | ★ | superset Desktop に対する **明確な差別化機能**。スマホ tailnet から worktree を直編集できる。 |
| Tasks (内部タスクトラッカー) | − | 外部 (Linear/GitHub) 連携が前提。 |
| Organization / Multi-host / OAuth | − | セルフホスト単機運用。 |
| Auto-update (`superset update` 相当) | − | 配布は cargo-dist / GitHub Releases。 |

### 1.3 agent-start CLI (独自設計)

`superset` CLI は模倣しない。本ツール独自の薄い CLI を提供する。原則: ローカルの host server に HTTP で問い合わせるシンクライアント。

```
agent-start start    [--bind <addr>] [--port <n>] [--daemon]   ホスト起動
agent-start stop                                                ホスト停止
agent-start status                                              稼働状況

agent-start project list
agent-start project add <path>
agent-start project remove <id>

agent-start workspace list   [--project <id>]
agent-start workspace create [--project <id>] --branch <name>
                              [--from <base>] [--agent <preset>]
                              [--prompt <text>]
agent-start workspace open   <id>                              ブラウザ UI を開く
agent-start workspace delete <id> [--keep-branch]

agent-start agent list
agent-start agent run --workspace <id> --agent <preset>
                      [--prompt <text>]

# 共通: --json / --quiet / --help / --version
```

scope 外: `superset auth …` / `organization …` / `tasks …` / `automations …` などは持たない。

### 1.4 ファイルレイアウト (独自)

| パス | 役割 |
| --- | --- |
| `~/.config/agent-start/config.json` | グローバル設定 (既存を継続)。 |
| `~/.cache/agent-start/worktrees/<workspace>/` | worktree 既定置き場 (環境変数 `AGENT_START_WORKTREE_ROOT` で上書き可、既存仕様継続)。 |
| `~/.local/share/agent-start/host.db` | SQLite (workspace / pty history / port labels など)。`$XDG_DATA_HOME` 尊重。 |
| `~/.local/share/agent-start/runtime/manifest.json` | 起動中ホストの bind/port/PID を CLI が読む。 |
| `.agent-start/config.json` (リポ内) | setup/teardown/run scripts。 |
| `.agent-start/config.local.json` | gitignore 推奨のローカル上書き (`before` / `after` 追記または全置換)。 |
| `~/.config/agent-start/projects/<id>/config.json` | ユーザ override (リポを汚さずに setup を差し替え)。 |
| `.agent-start/ports.json` | ポートに付ける friendly label。 |
| 注入 env vars (setup/run/agent プロセス) | `AGENT_START_ROOT_PATH`, `AGENT_START_WORKSPACE_NAME`, `AGENT_START_WORKSPACE_PATH` |

旧 `~/.config/agent-start/preferences.json` 等は起動時にマイグレーション。

---

## 2. アーキテクチャ (移行後)

### 2.0 バイナリ分離方針

`agent-start` (CLI) と `agent-start-host` (常駐サーバ) を **別の Cargo binary crate** として配布する。1 つの cargo workspace 内に共存させ、共通の型は `agent-start-api` クレートで共有する。

**分離する理由**:

- CLI は host への薄い HTTP クライアント。`reqwest` + `clap` + `serde` だけで足り、`tokio` ランタイム本体や `axum` / `sqlx` / `git2` / `portable-pty` を引きずらない (バイナリサイズと起動時間の差が大きい)
- ユーザは「リモートホスト用に CLI だけ入れたい」ケースがある (例: 別マシンの agent-start を叩く運用) — その時に host server バイナリを一緒に DL させたくない
- 役割が明確に分かれるのでクラッシュ影響範囲が独立する。CLI のバグで host を巻き込まない
- セキュリティ表面積: host バイナリだけ setuid/capability を要求するケースに対応しやすい
- `cargo install agent-start` だけで CLI が入る形にできる

**やり取り**:
- `agent-start start` は内部で `agent-start-host` を fork/exec (PATH または同梱パスから)
- それ以外の subcommand は `~/.local/share/agent-start/runtime/manifest.json` から URL を読んで HTTP リクエスト

```
┌────────────────────────┐   HTTP/JSON + WebSocket
│  /front (Vite + React) │ ◄──────────────────────────┐
│  - Workspace list      │                            │
│  - Terminal (xterm.js) │                            │
│  - Diff viewer         │                            │
│  - Ports / Browser     │                            │
└────────────────────────┘                            │
                                                      │
┌─────────────────────────────────────────────────────┴────────────────┐
│  agent-start host server (Rust)                                      │
│                                                                      │
│  HTTP/WS layer (axum + tokio-tungstenite)                            │
│   ├ /v1/workspaces          (CRUD)                                   │
│   ├ /v1/projects                                                     │
│   ├ /v1/agents/run                                                   │
│   ├ /v1/ports               (SSE + 一括 GET)                         │
│   ├ /v1/git/{status,diff,stage,unstage}                              │
│   ├ /ws/terminal?workspace=…&tab=…                                   │
│   ├ /v/<workspaceId>/*      (code-server リバースプロキシ)           │
│   └ /v1/health, /v1/version                                          │
│                                                                      │
│  Domain (Rust crates 構成)                                           │
│   ├ workspace_manager        (worktree / branch / PR import)         │
│   ├ pty_manager              (portable-pty + tab + 永続化)           │
│   │   └ session store: 出力リングバッファ → SQLite flush             │
│   ├ port_scanner             (/proc/net/tcp* + /proc/<pid>/fd 走査)  │
│   ├ git_ops                  (git2 で diff/stage/status/worktree)    │
│   ├ agent_runner             (preset → cmd line → spawn into PTY)    │
│   ├ config_loader            (.agent-start 階層 merge)               │
│   └ state                    (sqlx + SQLite)                         │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│  agent-start CLI (Rust、別バイナリ)                                  │
│   - clap derive で subcommand                                        │
│   - reqwest で host の /v1/* を叩く                                  │
│   - `start` だけは agent-start-host を spawn                         │
│   - runtime/manifest.json から bind/port を解決                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 2.1 Rust クレート選定 (第一候補)

| 用途 | クレート | メモ |
| --- | --- | --- |
| 非同期ランタイム | `tokio` | |
| HTTP server | `axum` | tower middleware で auth/CORS/log |
| WebSocket | `axum::extract::ws` | バイナリフレーム必須 (xterm.js) |
| リバースプロキシ | `tower-http` + 手書き、または `pingora` 検討 | code-server を `/v/<id>/` で透過 |
| PTY | `portable-pty` (Wezterm 製) | Linux/mac 両対応 |
| プロセス監視 | `procfs` (+ `sysinfo` 補助) | child tree → listening port |
| Git | `git2` (libgit2 バインディング) | worktree 操作で `gix` も比較 |
| DB | `sqlx` (SQLite) | マイグレーション同梱 |
| シリアライズ | `serde` / `serde_json` | |
| CLI parse | `clap` (derive) | CLI バイナリ側 |
| HTTP client | `reqwest` (rustls) | CLI から host へ |
| ログ | `tracing` + `tracing-subscriber` | |
| テスト | `insta` (snapshot) + `assert_cmd` (CLI) | |

### 2.2 tmux 撤廃の設計

要件:
1. ブラウザを閉じても child プロセスは生存し続ける
2. 再接続時にスクロールバック + 動作中プロセスが見える
3. 1 workspace に複数タブ
4. リサイズ追従

設計:
- host server プロセス内に `PtyManager` を持ち、`(workspace_id, tab_id) → PtyHandle` を保持
- `PtyHandle` = `portable-pty::Child` + `MasterPty` + リングバッファ (直近 ~100k bytes はメモリ、それ以前は SQLite に flush)
- WebSocket 接続時にリングバッファを一括フラッシュ → 以降は live tail
- host server 自体は **systemd-user (`agent-start.service`)** で常駐させる運用を README で推奨
- それでもホストがクラッシュした場合に備え PID を SQLite に保存し、起動時に `/proc/<pid>/status` で生存確認 → 生きていればタブ復旧、死んでいれば履歴のみ保持

### 2.3 認証/露出範囲

- 既定で `127.0.0.1:<port>` バインド
- tailnet/LAN 公開時は `--bind 0.0.0.0` を明示
- API トークン (任意): `$AGENT_START_API_KEY` をヘッダ/Cookie で受ける

---

## 3. フェーズ別ロードマップ

### Phase 0 — 準備 (1〜2 日)
- [ ] `docs/ROADMAP.md` (本書) 確定
- [ ] GitHub Issue 起票 (5. 参照)
- [ ] Rust workspace スケルトン作成 (`/server-rs/`)
- [ ] CI: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo audit`

**完了条件**: `cargo run -p agent-start-host -- start` で空の HTTP `/v1/health` が返る。

### Phase 1 — Rust 上で既存機能パリティ (1〜2 週)
- [ ] `projects` / `workspaces` の CRUD と worktree 操作 (`git2`)
- [ ] `PtyManager` 実装 + `/ws/terminal` (リングバッファ + 再接続)
- [ ] `git status` / `git diff` API
- [ ] `config.json` (既存 `~/.config/agent-start/config.json`) ローダ + `.agent-start/config.json` ローダ
- [ ] Node サーバ (`server.mjs` + `app/api`) を撤去。`package.json` から server 系依存を削除
- [ ] 既存 Next.js UI を `/front` に Vite + React で **同等の画面で** 再構築

**完了条件**:
- `npm --prefix front run dev` で旧 UI と同等の操作ができる
- `tmux` バイナリ不要で起動できる
- 既存ユーザの `~/.config/agent-start/config.json` が自動マイグレーションで動く

### Phase 2 — agent-start CLI + ファイルレイアウト整理 (1 週)
- [ ] `clap` で本ツール独自の CLI を実装
- [ ] `~/.local/share/agent-start/host.db` + `runtime/manifest.json` を確立
- [ ] `.agent-start/config.json` (setup/teardown/run) の実行 + env vars 注入
- [ ] `config.local.json` の `before` / `after` マージ仕様
- [ ] ユーザ override (`~/.config/agent-start/projects/<id>/config.json`)

### Phase 3 — agents / ports / diff staging (1 週)
- [ ] Agent preset スキーマ。Claude/Codex/Cursor/Gemini/OpenCode/Pi/Amp の初期 preset 同梱
- [ ] `agent run` で workspace の PTY に prompt を注入
- [ ] Port scanner (`/proc/net/tcp*` × `/proc/<pid>/fd`) + `.agent-start/ports.json` ラベル
- [ ] Diff viewer の hunk-level staging API (`POST /v1/git/stage`)

### Phase 4 — VSCode Web UI 統合 (1 週)
- [ ] code-server を子プロセスとして spawn (workspace ごと)
- [ ] host server がリバースプロキシ (`/v/<workspaceId>/`) で透過
- [ ] UI に「VSCode で開く」ボタン
- [ ] (詳細は Issue #9 / 本書 §4)

### Phase 5 — Automations / In-app browser (任意)
- [ ] RRULE スケジューラ (`rrule` crate)
- [ ] iframe ベースの簡易プレビューペイン

---

## 4. VSCode Web UI 統合 (機能設計)

別 Issue (#9) として起票済み。Web セルフホスト戦略の **目玉差別化機能**。

**要件**:
- workspace の worktree パスを **ブラウザ上の VSCode 風 IDE** で開く
- agent-start UI 内に「VSCode で開く」ボタンを置き、新規タブで開く
- スマホ tailnet 経由の利用が前提なのでデスクトップ Code 起動はサブ手段

**v1 採用: code-server (Coder)**。MIT、marketplace 互換 (OpenVSX)、安定。

**仕組み**:
- agent-start host が workspace 起動時 (または初回ボタン押下時) に `code-server` 子プロセスを spawn
  - `code-server --auth none --bind-addr 127.0.0.1:<auto> --user-data-dir <…> --extensions-dir <…> <worktree-path>`
- axum + tower-http で `https://<host>/v/<workspaceId>/` → `127.0.0.1:<port>` をリバースプロキシ (WebSocket 透過)
- 認証: agent-start のセッション Cookie を再利用
- workspace 削除と連動して code-server プロセスを kill

---

## 5. GitHub Issue 一覧

`imanect-labs/agent-start` に起票済み:

1. #4  [Epic] セルフホスト型 superset 代替の構築
2. #5  Rust バックエンドへの置換 (axum + tokio)
3. #6  tmux 撤廃: 自前 PTY マネージャ (portable-pty + 永続化)
4. #7  Next.js → /front (Vite + React) への分離
5. #8  agent-start CLI (clap) + ファイルレイアウト整理
6. #9  VSCode Web UI 統合 (code-server 同梱 + リバースプロキシ)
7. #10 agent preset スキーマ + 7 種 CLI 同梱
8. #11 Ports auto-detection (`/proc` 走査) + `.agent-start/ports.json`
9. #12 Diff Viewer staging API (hunk-level)

---

## 6. リスクと未確定事項

- **superset Desktop に追いつくのは Web UI の表現力**。Electron 専用機能 (グローバルショートカット、OS ネイティブ通知、Finder/Explorer 連携) は諦めるか PWA でカバー
- **PTY 永続化の冪等性**。host server クラッシュ時の子プロセス孤児化を防ぐ運用 (systemd-user 推奨) をドキュメント化
- **マイグレーション**。`~/.config/agent-start/config.json` 利用者のために 1 リリースだけ自動 import を残す
- **モバイル UX**。`/front` 再構築時に既存スマホ UI を回帰させないこと
- **code-server の同梱戦略**。配布バイナリには含めず、初回利用時に DL する想定 (`SUPPORTED_CODE_SERVER_VERSION` をピン留め)

---

## 7. 当座のアクションアイテム

1. 本ドキュメントを `main` にマージ
2. Issue #4–#12 の優先順位確認 (Phase 0 → 1 → 2)
3. `/server-rs/` を作って Phase 0 着手
4. `/front` の雛形 (Vite + React + TS) を作る

最終更新: 2026-05-18

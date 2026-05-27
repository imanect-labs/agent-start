# agent-start ロードマップ

## ゴール

agent-start を **Web で動く、セルフホスト可能なエージェント開発ハブ** に作り直す。

参考にした macOS Desktop アプリがあるが、agent-start はそれを **Linux サーバ上で常駐させ、tailnet 経由でブラウザ (PC / スマホ) から使う** ことを前提にした OSS Web 実装を目指す。同等の機能セット (workspace = worktree、agent 並列実行、永続ターミナル、ポート自動検出、diff viewer 等) を Web UI で享受できれば良い。

設計上の判断:

- **Desktop 製品とのワイヤ互換 (CLI / HTTP API / レガシーホームディレクトリ) は追わない**。参考にした Desktop 製品のソースは Elastic License 2.0 で借用に制約があり、また Desktop 製品の SDK / MCP / CLI スクリプトとの互換性を維持するメリットも薄い。
- **Rust バックエンド**: 速度・メモリ安全性・小さな静的バイナリでの配布を理由に Node/Next.js の API・サーバ部を置換する。CLI とサーバは **別バイナリ** (`agent-start` と `agent-start-host`) として分離 (理由は §2.0)。
- **tmux 撤廃**: PTY 多重化と「切断後も生存」を Rust ホストプロセス内で完結 (`portable-pty` + 自前永続化)。tmux 外部依存をなくす。
- **/front 分離**: 既存 Next.js を捨て、`/front` に Vite + React の薄い SPA を新規構築。バックエンドとは HTTP/WebSocket だけで会話する。
- **モバイル/タブレットでの利用品質**を差別化のキモにする (既存 Desktop 製品は macOS Desktop のみ)。

## 1. 機能スコープ

参考にする Desktop 製品の機能セットを下敷きにしつつ、Web セルフホスト前提で取捨選択する。`★` = v1 必須 / `☆` = v2 / `−` = 非スコープ。

### 1.1 提供形態

| 参考とする Desktop 製品の形態 | agent-start での扱い |
| --- | --- |
| Desktop IDE (macOS Electron) | − 採らない。 |
| Host Server (`<参考 daemon>`、127.0.0.1) | ★ Rust 常駐プロセス。既定は `127.0.0.1`、tailnet/LAN 公開は `--bind` 明示。 |
| CLI (参考の単一バイナリ) | ★ Rust 製 `agent-start` CLI。サブコマンドは参考 CLI を模さず、本ツール独自の語彙で揃える。 |
| リモートワークスペース横断 (参考 Relay 機能) | − tailnet で代替。 |
| MCP server デプロイ | ☆ v2 で検討。 |

### 1.2 機能 (参考 Desktop 製品から取り込む項目)

`★` = v1 必須 / `☆` = v2 / `−` = 非スコープ。参考 Desktop 製品の機能を可能な限り粒度を揃えて列挙する。

#### 1.2.1 Project (リポジトリ) 管理

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| Project の **clone** 登録 (URL → 取得 → 配下に保存) | ★ | `clone_url` 指定で `~/.agent-start/worktrees/<projectId>/.repo/` に bare/clone。 |
| Project の **import** 登録 (既存ローカル repo を pin) | ★ | 旧 `roots` 相当はこちらに統合。`roots` 設定でディレクトリスキャンも可 (opt-in)。 |
| Project の **再ロケート** (パス引っ越し検出) | ☆ | `--allow-relocate` 相当。 |
| Project の **削除** (登録解除 / オプションでファイル削除) | ★ | |
| Project list / detail / 編集 (名前変更、デフォルト agent) | ★ | |
| Project レベル設定 `.agent-start/config.json` (setup/teardown/run) | ★ | §1.5 参照。 |

#### 1.2.2 Workspace (= 1 git branch = 1 git worktree)

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| Workspace 作成: **新規ブランチ** (base から切る) | ★ | `--branch <new> --from <base>`。base は省略時 project の HEAD ブランチ。 |
| Workspace 作成: **既存ブランチを resume** | ★ | 既存ブランチ名を渡せば worktree だけ追加。 |
| Workspace 作成: **PR import** (`gh pr checkout`) | ★ | `--pr <number>` で fetch + worktree。 |
| **base ブランチを最新化してから worktree を作る** | ★ | 既定 ON。`git fetch <remote> <base>` → fast-forward → worktree 作成。`--no-refresh-base` で skip 可。 |
| **初回 prompt の注入** (作成と同時に agent を spawn しプロンプトを送る) | ★ | UI の起動シート / CLI の `--prompt <text>`。spawn 完了後 PTY に書き込む。 |
| **添付ファイル** (`--attachment <path>` で worktree に同梱、agent に位置を通知) | ★ | スクショ・仕様書を渡す用途。worktree 内 `.agent-start/attachments/` に複製、prompt 末尾にパスを追加。 |
| Workspace **rename** | ★ | UI からも CLI からも。 |
| Workspace **delete** (worktree 削除 + 連動 branch 削除をオプション) | ★ | `--keep-branch` でブランチは残す。 |
| Workspace 一覧 + **検索 / 絞り込み** (project / status / branch 名) | ★ | |
| Workspace **状態表示** | | |
| 　・ahead / behind remote (↑N / ↓N) | ★ | `git rev-list --left-right` 等で算出。 |
| 　・PR 紐付き状態 (open / merged / closed) | ★ | `gh pr view --json`。 |
| 　・CI checks サマリ (pass / fail / pending) | ★ | `gh pr checks --json`。 |
| 　・deployment preview URL | ☆ | PR labels や `gh pr view` の出力から拾えれば。 |
| Workspace を **外部 IDE で開く** (`code` / `cursor` / JetBrains 等の deep link) | ★ | OS 側に CLI があれば exec。サーバホスト経由で client に deep link 配信する経路も検討。 |
| Workspace を **ブラウザ VSCode (code-server)** で開く | ★ | §4。差別化機能。 |

#### 1.2.3 ターミナル

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| 1 workspace = N tabs (multi-tab) | ★ | |
| **切断後も生存** (process は host 内で生き続ける) | ★ | tmux 撤廃して PtyManager 内に保持 (§2.2)。 |
| **スクロールバック保持** (再接続時にリプレイ) | ★ | リングバッファ + SQLite flush。 |
| リサイズ追従 | ★ | |
| キーボード操作 (新規 tab / 閉じる / tab 切替 / コピー&ペースト) | ★ | スマホでは長押しメニューで代替。 |
| URL / ファイルパスの **Cmd/Ctrl+Click で開く** | ☆ | ファイルは worktree 相対で解決し VSCode/code-server に飛ばす。 |
| Per-workspace の **ENV 注入** (`AGENT_START_ROOT_PATH`, `AGENT_START_WORKSPACE_NAME`, `AGENT_START_WORKSPACE_PATH`) | ★ | setup/run/agent プロセス共通。 |
| ターミナル preset (フォント / 色) | ☆ | |

#### 1.2.4 Agent ランナー

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| Preset: Claude Code / Codex / Cursor Agent / Gemini CLI / OpenCode / Pi / Amp Code | ★ | §1.6。 |
| **Auto-run トグル** (即時 exec か、コマンド入力欄にステージするだけか) | ★ | 参考 Desktop の "Auto-run Command"。 |
| **prompt-aware command variant** (prompt の有無で起動引数を切替) | ★ | preset の `args_template_with_prompt` を別途持つ。 |
| **task prompt テンプレート** (タスク連携起動時のテンプレ文字列) | ☆ | |
| **モデル override** (preset ごとに `--model` 等を上書き) | ★ | UI の「設定→Agents」から。 |
| 添付ファイル (作成済み workspace に対しても `agent run --attachment`) | ★ | 既存セッションに prompt 追加注入。 |
| preset の **enable / disable** (UI のランチャーから隠す) | ★ | |
| preset を **デフォルトに戻す** | ★ | |

#### 1.2.5 設定スクリプト (setup / teardown / run)

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| Repo 内 `.agent-start/config.json` で **setup** / **teardown** / **run** を宣言 | ★ | §1.5。 |
| **setup** は workspace 作成時にブロックして実行 | ★ | 失敗時は作成中止 + worktree クリーン。 |
| **teardown** は workspace 削除時に実行 | ★ | 失敗しても削除は完遂。 |
| **run** は on-demand (UI の「Run」ボタン)、専用 terminal pane で表示、**再起動可能** | ★ | dev server 想定。 |
| **`.agent-start/config.local.json`** (gitignore 推奨) で setup/teardown/run を `before` / `after` で拡張 or 全置換 | ★ | 参考 Desktop 互換相当の挙動。 |
| **`~/.agent-start/projects/<projectId>/config.json`** でユーザ override | ★ | リポを汚さずに setup を差し替え。 |

#### 1.2.6 Diff Viewer

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| 変更ファイル一覧 (modified / staged / untracked) | ★ | |
| split / unified 切替 | ★ | |
| **hunk 単位 stage / unstage** | ★ | `git apply --cached` ベース。Issue #12。 |
| **行単位 stage** | ☆ | hunk から派生で実装。 |
| **focus mode** (1 ファイル詳細レビュー) | ★ | UI 上で全画面。 |
| 行選択 → コピー (コンテキストメニュー) | ★ | |
| **ファイル間ナビゲーション** (前後 / Jump to file) | ★ | キーボード対応。 |
| commit / discard (UI から実行) | ★ | safety: discard は確認シート必須。 |

#### 1.2.7 Ports

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| 自動検出 (`/proc/net/tcp*` × プロセスツリー) | ★ | Issue #11。 |
| Workspace 単位でグルーピング | ★ | |
| ポートをクリック → 元 terminal にフォーカス | ★ | UI 動作。 |
| **新規タブで開く / in-app browser で開く** | ★ | |
| **プロセス終了 (kill)** | ★ | SIGTERM → 1.5s → SIGKILL。 |
| **ラベル付与** (`.agent-start/ports.json`) | ★ | JSON 壊れ時は warn して無視。 |
| 変化を **SSE で push** + フォールバックの全件 refetch | ★ | |

#### 1.2.8 In-app browser

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| 検出された port を iframe でプレビュー | ☆ | CSP/X-Frame で開けない場合は別タブにフォールバック。 |
| URL bar (リロード / 戻る / 別タブで開く) | ☆ | |

#### 1.2.9 VSCode Web UI (差別化機能)

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| **code-server を workspace ごとに spawn**、host がリバースプロキシ | ★ | §4 / Issue #9。 |
| 認証は agent-start のセッションを Cookie で再利用 | ★ | |
| code-server のバージョンピン留め (初回 DL or PATH 既存利用) | ★ | |
| 同一 workspace の再オープンでインスタンス共有 | ★ | |

#### 1.2.10 Automations (v2)

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| RRULE (RFC 5545) スケジュール | ☆ | |
| 各 run = workspace snapshot として保存 | ☆ | |
| run history (dispatched / skipped (offline) / failed) | ☆ | |
| MCP scopes (有効ツール限定) | ☆ | MCP 実装と連動。 |
| 「冪等な prompt 設計」を docs に明示 (at-least-once) | ☆ | |

#### 1.2.11 全体 UX

| 機能 | 必須度 | 備考 |
| --- | --- | --- |
| キーボードショートカット (新規 workspace / 開く / tab 操作 / コマンドパレット) | ★ | 参考 Desktop の `⌘O / ⌘T / ⌘W / ⌘1-9` 相当を Web で。 |
| **コマンドパレット** (`Ctrl+K` / `Cmd+K`) | ★ | 新規 workspace / agent run / open in code-server を即起動。 |
| テーマ (既存 ThemeProvider 継続、light/dark) | ★ | |
| Toast / 確認シート | ★ | 既存を `/front` で再現 (UX 改善ありき)。 |
| 設定 UI (preset / scripts / preferences) | ★ | |
| **モバイル/タブレット UX を再設計** (左右ペイン → 縦スタック、片手操作) | ★ | 既存 UI の破壊的変更を許容 (むしろ作り直す)。 |

#### 1.2.12 非スコープ (v1)

| 項目 | 理由 |
| --- | --- |
| 参考 Desktop の CLI / HTTP API / ホームディレクトリ互換 | 互換維持のメリットが薄く ELv2 制約もあるため。 |
| 参考 Desktop Relay (リモート横断) | tailnet で代替。 |
| 内部 Tasks トラッカー | Linear / GitHub Issues 連携が前提。 |
| Organization / Multi-host / OAuth | セルフホスト単機運用。 |
| Auto-update | cargo-dist / GitHub Releases に委ねる。 |
| 外部 AI provider 設定 UI (OpenRouter / Bedrock / Vertex 等) | wrap している CLI 側の設定で行う。 |

### 1.3 agent-start CLI (独自設計)

参考 Desktop の CLI は模倣しない。本ツール独自の薄い CLI を提供する。原則: ローカルの host server に HTTP で問い合わせるシンクライアント (詳細は §2.0)。

```
agent-start start    [--bind <addr>] [--port <n>] [--daemon]   ホスト起動
agent-start stop                                                ホスト停止
agent-start status                                              稼働状況

# project
agent-start project list
agent-start project add  --import <path>                       既存ローカル repo を登録
agent-start project add  --clone <url> [--parent-dir <path>]   git clone して登録 (既定 ~/.agent-start/worktrees/<id>/.repo/)
agent-start project remove <id> [--delete-files]
agent-start project rename <id> --name <new>

# workspace
agent-start workspace list   [--project <id>] [--status <s>] [--search <q>]
agent-start workspace create --project <id>
                              ( --branch <new> [--from <base>]
                              | --resume <existing-branch>
                              | --pr <number> )
                              [--no-refresh-base]
                              [--agent <preset>] [--prompt <text>]
                              [--attachment <path> ...]
agent-start workspace rename <id> --name <new>
agent-start workspace open   <id> [--editor code|cursor|jetbrains|web]
agent-start workspace delete <id> [--keep-branch]
agent-start workspace status <id>                              ahead/behind/PR/CI を JSON で

# agent
agent-start agent list
agent-start agent run --workspace <id> --agent <preset>
                      [--prompt <text>] [--attachment <path> ...]

# scripts
agent-start run set --workspace <id> --name <key>              .agent-start の run スクリプト起動
agent-start run stop --workspace <id> --name <key>

# diff / ports は HTTP 直叩きが主で CLI は後追い
# 共通: --json / --quiet / --help / --version
```

scope 外: `auth …` / `organization …` / `tasks …` / `automations …` (v2)。

### 1.4 ファイルレイアウト (独自)

`~/.agent-start/` をデータルートに統一する。`$AGENT_START_HOME` で全体を上書き可。

| パス | 役割 |
| --- | --- |
| `~/.agent-start/` | データルート (全部ここに集約)。`$AGENT_START_HOME` で上書き可。 |
| `~/.agent-start/worktrees/<projectId>/.repo/` | clone モードで取得した project の git ディレクトリ (bare or non-bare)。 |
| `~/.agent-start/worktrees/<projectId>/<branch>/` | **worktree の既定置き場 (新規 default)**。env `AGENT_START_WORKTREE_ROOT` で base を上書き可。 |
| `~/.agent-start/host.db` | SQLite (project / workspace / pty history / port labels / agent presets cache)。 |
| `~/.agent-start/runtime/manifest.json` | 起動中ホストの bind/port/PID/開始時刻。CLI が読む。 |
| `~/.agent-start/projects/<projectId>/config.json` | ユーザ override (リポを汚さずに setup を差し替え)。 |
| `~/.agent-start/presets/*.toml` | ユーザ作成の agent preset (同梱 preset を override)。 |
| `~/.agent-start/logs/*.log` | host のログ (rotate)。 |
| `~/.config/agent-start/config.json` | グローバル設定 (XDG)。テーマ / デフォルト agent / `roots` / バインドアドレス等。 |
| `.agent-start/config.json` (リポ内) | setup / teardown / run スクリプト。 |
| `.agent-start/config.local.json` | gitignore 推奨のローカル上書き (`before` / `after` 追記または全置換)。 |
| `.agent-start/ports.json` (リポ内) | ポートに付ける friendly label。 |
| `<worktree>/.agent-start/attachments/` | 作成時に渡された添付ファイルの保存先。 |
| 注入 env vars | `AGENT_START_ROOT_PATH` (project の repo root), `AGENT_START_WORKSPACE_NAME`, `AGENT_START_WORKSPACE_PATH`, `AGENT_START_HOST_URL` |

**roots 設定の役割変更**:
- 旧: `~/dev` を scan して repo を自動列挙 (project の概念無し)
- 新: 既定 `roots: []`。指定された場合は scan して **import 候補** として UI に出す (ワンクリックで project に登録)
- 既存ユーザの `~/.config/agent-start/config.json` に `roots: ["~/dev"]` があれば起動時にマイグレーションして同等の挙動

旧 `~/.config/agent-start/preferences.json` / `~/.cache/agent-start/worktrees/` は起動時にマイグレーション。

### 1.5 Workspace ライフサイクル詳細

#### 1.5.1 作成 (Create)

入力:
- `project_id`
- 起動モード = `new_branch` | `resume` | `pr_import`
- `branch` (新規 or 既存) / `from_base` (新規時の元) / `pr_number` (PR モード時)
- `refresh_base` (既定 true)
- `agent_preset` (省略可)
- `prompt` (`agent_preset` 指定時は必須 / 単独でも UI から渡せる)
- `attachments[]` (パス配列)

ステップ:
1. **project の repo root を確定** (clone 済みなら `worktrees/<projectId>/.repo/`、import なら登録パス)
2. **base ブランチ refresh** (`refresh_base=true` のとき)
   - `git -C <repo-root> fetch <remote> <base>`
   - `<base>` がローカルに無ければ作る、あれば fast-forward 試行 (非 ff は warn 出してそのまま継続)
3. **branch 用意**
   - `new_branch`: `git worktree add -b <branch> <worktree-path> <base>`
   - `resume`: `git worktree add <worktree-path> <branch>`
   - `pr_import`: `gh pr checkout <number> --repo <slug>` を一時 worktree でやり、HEAD 名を取得して通常 worktree に組み直す (gh CLI 必須)
4. **worktree path 決定**: `~/.agent-start/worktrees/<projectId>/<sanitized-branch>/` (重複時は suffix `-2`, `-3`...)
5. **attachment コピー**: `<worktree>/.agent-start/attachments/<original-name>` に複製、prompt 末尾に `[attached: ...]` を追加
6. **setup スクリプト実行** (§1.5.4) — 失敗時は worktree を rollback (`git worktree remove --force` + branch 削除)、エラーを UI に表示
7. **DB 登録** (`host.db.workspaces`) — id, project_id, branch, worktree_path, status=`ready`
8. **agent spawn** (preset 指定時)
   - 新規 PTY tab を 1 つ作り `cwd=<worktree-path>`、`ENV` に §1.2.3 を注入
   - preset の command を起動
   - **prompt 注入**: preset の `prompt_arg` が定義されていればそれ経由 (例 `claude -p "<prompt>"`)、定義されていなければ PTY 起動 → readiness 検出 (一定ダンプ後) → prompt を `term.write` で流す
   - `auto_run=false` の場合は prompt を入力欄に「stage」するだけ (PTY に Enter を送らず、最後の改行を消す)

#### 1.5.2 状態取得 (Status)

定期 (5–10 秒) と on-demand で計算:

- `git -C <worktree> rev-list --left-right --count <branch>...<upstream>` → ahead / behind
- `gh pr view --json number,state,statusCheckRollup,url --repo <slug> <branch>` → PR メタ
- PTY 状態 (idle / running / exited) は PtyManager から取得
- 結果は `host.db.workspace_status` にキャッシュ、SSE `/v1/workspaces/events` で push

#### 1.5.3 更新 (Update)

- `rename`: DB 上の name を変更 (branch 名・worktree path は変えない)
- `agent rerun`: 既存 PTY tab に prompt 追加注入 (新規 tab を生やすかは UI 設定)
- `run script start/stop`: 専用 PTY tab で `.agent-start/config.json` の `run.<key>` を起動 (再起動可能)

#### 1.5.4 setup / teardown / run スクリプト

`.agent-start/config.json` のスキーマ (例):

```json
{
  "setup": [
    { "name": "install", "cmd": "pnpm install" }
  ],
  "teardown": [
    { "name": "cleanup", "cmd": "pnpm clean" }
  ],
  "run": [
    { "name": "dev", "cmd": "pnpm dev", "ports": [3000] }
  ]
}
```

マージ規則 (優先度高 → 低):
1. `~/.agent-start/projects/<projectId>/config.json` (ユーザ override)
2. `<worktree>/.agent-start/config.local.json` (`before` / `after` で挿入 or 全置換)
3. `<worktree>/.agent-start/config.json` (commit 済み team default)

実行環境:
- cwd = workspace の worktree
- ENV = §1.2.3 の注入 + 通常の継承
- setup は **直列実行 + ブロック**、log は host が capture して UI に出す
- run は **再起動可能**、UI の Run ボタンに紐付き、stop で SIGTERM (1.5s 後 SIGKILL)

#### 1.5.5 削除 (Delete)

1. 当該 workspace の全 PTY tab を kill (SIGTERM → 1.5s → SIGKILL)
2. `run` で立てたプロセスも同様に kill
3. **teardown スクリプト実行** (失敗しても継続、log は保持)
4. `git worktree remove --force <path>`
5. `--keep-branch` でなければ `git branch -D <branch>`
6. attachment / `.agent-start/attachments/` ごと削除
7. DB 上の workspace 行 + 関連 pty_history / port_labels を削除

### 1.6 Agent preset の詳細スキーマ

`presets/<id>.toml` (同梱 + ユーザ override で merge):

```toml
id              = "claude"
label           = "Claude Code"
enabled         = true
command         = "claude"
args            = []                      # 共通引数
args_with_prompt = []                     # prompt 指定時のみ追加される引数
prompt_arg      = "-p"                    # prompt をコマンド引数で渡す場合のフラグ。未指定なら PTY stdin に流す
prompt_template = "{prompt}"              # placeholder 置換可
auto_run        = true                    # false なら入力欄に stage するだけ
model_override  = ""                      # 空なら preset 既定モデル
skip_permissions_flag = "--dangerously-skip-permissions"   # オプション (旧 clis 互換)
env             = { ANTHROPIC_API_KEY = "${ANTHROPIC_API_KEY}" }
```

同梱 preset (Phase 3):

| id | command | prompt_arg | auto_run | 備考 |
| --- | --- | --- | --- | --- |
| claude | `claude` | `-p` | true | `--dangerously-skip-permissions` も sup |
| codex | `codex` | (stdin) | true | `--full-auto` フラグ既定 |
| cursor | `cursor-agent` | `--prompt` | true | |
| gemini | `gemini` | `-p` | true | |
| opencode | `opencode` | `-p` | true | |
| pi | `pi` | (stdin) | true | minimal harness |
| amp | `amp` | `-p` | true | |

UI からの override は `~/.agent-start/presets/<id>.toml` に書き出し、reset で削除。

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

### Phase 1 — Rust 上で機能パリティ + UI 再設計 (1〜2 週)
- [ ] `projects` / `workspaces` の CRUD と worktree 操作 (`git2`)
- [ ] **worktree 既定パスを `~/.agent-start/worktrees/<projectId>/<branch>/` に変更**
- [ ] `PtyManager` 実装 + `/ws/terminal` (リングバッファ + 再接続)
- [ ] `git status` / `git diff` API
- [ ] `config.json` (既存 `~/.config/agent-start/config.json`) ローダ + `.agent-start/config.json` ローダ
- [ ] Node サーバ (`server.mjs` + `app/api`) を撤去。`package.json` から server 系依存を削除
- [ ] **`/front` を Vite + React で新規構築** (UI は破壊的変更を許容して **再設計**: モバイル/タブレット片手操作前提、コマンドパレット導入、レイアウト見直し)
- [ ] 既存ユーザの `~/.config/agent-start/config.json` + `~/.cache/agent-start/worktrees/` を新パスへ自動マイグレーション

**完了条件**:
- `npm --prefix front run dev` で新 UI で旧機能 (workspace 作成/起動/削除/diff) が回る
- `tmux` バイナリ不要で起動できる
- worktree が `~/.agent-start/worktrees/` 配下に作られる
- 旧設定ユーザがマイグレーションで自然に移行できる

### Phase 2 — agent-start CLI + ファイルレイアウト整理 (1 週)
- [ ] `clap` で本ツール独自の CLI を実装
- [ ] `~/.local/share/agent-start/host.db` + `runtime/manifest.json` を確立
- [ ] `.agent-start/config.json` (setup/teardown/run) の実行 + env vars 注入
- [ ] `config.local.json` の `before` / `after` マージ仕様
- [ ] ユーザ override (`~/.config/agent-start/projects/<id>/config.json`)

### Phase 3 — workspace 機能拡張 + agents / ports / diff staging (1.5 週)
- [ ] **base ブランチ refresh** + `--no-refresh-base` フラグ
- [ ] **PR import モード** (`gh pr checkout` 連携)
- [ ] **初回 prompt 注入** + **attachment** (worktree への複製 & prompt 付加)
- [ ] **workspace status**: ahead / behind / PR / CI、SSE で push
- [ ] **コマンドパレット (Cmd+K)** for: 新規 workspace / agent run / open in VSCode / run script
- [ ] **外部 IDE で開く** (`code` / `cursor` / JetBrains の deep link 起動)
- [ ] **setup / teardown / run スクリプト** 実行 + `config.local.json` `before/after` マージ + ユーザ override
- [ ] Agent preset スキーマ + 7 種同梱 (§1.6)、UI から enable/disable/reset
- [ ] Port scanner (`/proc/net/tcp*` × `/proc/<pid>/fd`) + `.agent-start/ports.json` ラベル
- [ ] Diff viewer の hunk-level staging API (`POST /v1/git/stage` / `unstage`) + UI

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

1. #4  [Epic] セルフホスト型 Web IDE の構築
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

- **参考 Desktop に追いつくのは Web UI の表現力**。Electron 専用機能 (グローバルショートカット、OS ネイティブ通知、Finder/Explorer 連携) は諦めるか PWA でカバー
- **PTY 永続化の冪等性**。host server クラッシュ時の子プロセス孤児化を防ぐ運用 (systemd-user 推奨) をドキュメント化
- **マイグレーション**。`~/.config/agent-start/config.json` 利用者のために 1 リリースだけ自動 import を残す
- **モバイル UX**。既存スマホ UI の見た目には縛られない (むしろ作り直す) が、片手操作・ソフトキーボード共存・タブ切替の踏みやすさ等は新 UI の合意点としてレビューで担保する
- **code-server の同梱戦略**。配布バイナリには含めず、初回利用時に DL する想定 (`SUPPORTED_CODE_SERVER_VERSION` をピン留め)
- **`gh` CLI 依存**。PR import / status 取得で必須。host server 起動時に `gh auth status` を warn 出力

---

## 7. 当座のアクションアイテム

1. 本ドキュメントを `main` にマージ
2. Issue #4–#12 の優先順位確認 (Phase 0 → 1 → 2)
3. `/server-rs/` を作って Phase 0 着手
4. `/front` の雛形 (Vite + React + TS) を作る

最終更新: 2026-05-18 (v2: 機能列挙の詳細化 / worktree 既定パス変更 / UI 再設計許容)

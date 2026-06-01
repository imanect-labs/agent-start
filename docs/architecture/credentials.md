# ユーザー別サブスクリプション / 資格情報の管理設計

親: [multiuser-rfc.md](./multiuser-rfc.md)　関連: [isolation.md](./isolation.md), [data-scoping.md](./data-scoping.md)

agent-start が起動する `claude` / `codex` CLI の認証は、**agent-start アプリ自体の認証（[auth.md](./auth.md)）とは別レイヤー**である。各 CLI は自分の認証情報をホームディレクトリ配下に保存する。マルチユーザーでは、各ユーザーが**自分のサブスクリプション（または API キー）を持ち込み**、それが他ユーザーと分離されている必要がある。

## CLI の認証の仕組み（前提）

| CLI | サブスク認証 | 保存先（HOME 配下） | API キー代替 |
| --- | --- | --- | --- |
| Claude Code | `claude login`（OAuth, Pro/Max） | `~/.claude/`（`.credentials.json` 等。macOS は Keychain の場合あり） | `ANTHROPIC_API_KEY` |
| Codex | `codex login`（OAuth, ChatGPT Plus/Pro） | `~/.codex/auth.json` | `OPENAI_API_KEY` |

いずれも認証情報は **`$HOME` 配下のファイル**として永続化される（OAuth トークンは CLI 自身が管理）。

## 現状（単一ユーザー前提の核）

- spawn されるエージェントは**ホストプロセスの環境を継承**する。`server-rs/bin/agent-start-host/src/sessions.rs::launch_env` は `AGENT_START_ROOT_PATH` / `WORKSPACE_NAME` / `WORKSPACE_PATH` / `TERM` の 4 つだけを設定し、**`HOME` を設定していない**。結果、全セッションがホストユーザーの `~/.claude/` / `~/.codex/` の**単一ログインを共有**する。
- `server-rs/crates/workspace-manager/src/lib.rs::mark_claude_trusted` は `dirs::home_dir()`（= ホストの HOME）の `~/.claude.json` を読み書きするため、全ユーザーが同一ファイルを上書きし合う。
- `config.example.json` / `config-loader/src/config.rs` の `CliConfig` に env 注入機構はない（`docs/ROADMAP.md` に `env` フィールドの計画あり、未実装）。

## 設計: per-user HOME = per-user サブスク分離

[isolation.md](./isolation.md) でユーザーごとに `HOME=~/.agent-start/users/<uid>/home` を割り当てる設計を活用すると、**各 CLI の資格情報が自動的にユーザー単位で分離**される。OAuth サブスクの場合、追加の秘密ストアは不要。

### モード 1: OAuth サブスクリプション（推奨）
- 各ユーザーが自分のターミナルセッションで一度だけ `claude login` / `codex login` を実行（OAuth はブラウザ/デバイスコードフロー。リモートでも PTY 経由で URL/コードが表示され完結できる）。
- トークンは各自の per-user HOME（`users/<uid>/home/.claude/` 等）に保存され、以降のセッションで再利用。各自の Pro/Max/Plus シートをそのまま使う。
- agent-start はトークンを**保存・代理しない**（CLI と HOME に委ねる）。やることは HOME を正しく per-user に向けることだけ。

### モード 2: API キー
- API 課金を使うユーザー向けに、`ANTHROPIC_API_KEY` / `OPENAI_API_KEY` を**ユーザー単位で暗号化保存し、spawn 時に env 注入**する。
- 新テーブル `user_credentials`（暗号化 at rest）:
  ```sql
  CREATE TABLE user_credentials (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    cli        TEXT NOT NULL,          -- 'claude' | 'codex' | '*'
    env_key    TEXT NOT NULL,          -- 'ANTHROPIC_API_KEY' 等
    enc_value  BLOB NOT NULL,          -- マスターキーで暗号化（AGENT_START_SECRET_KEY / age 等）
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (user_id, cli, env_key)
  );
  ```
- マスターキーは env（`AGENT_START_SECRET_KEY`）等から供給。OAuth トークンはここに入れない（CLI が HOME で管理するため）。

## 必要なコード変更

1. **`launch_env`（`sessions.rs`）で `HOME` を per-user に明示設定** + `paths.rs` の `user_home(uid)` を使い `AGENT_START_HOME` 等も per-user に（[data-scoping.md](./data-scoping.md)）。これが最重要修正。
2. **`workspace-manager::mark_claude_trusted(dir)` を `mark_claude_trusted(home, dir)` に**変更し、per-user HOME の `~/.claude.json` を編集（現状の `dirs::home_dir()` 固定を撤廃）。呼び出し側（`http/sessions.rs`）が `AuthUser` から解決した per-user HOME を渡す。
3. **`CliConfig` に `env: Map<String,String>` を追加**し、`${env:VAR}` / `${secret:KEY}` 置換をサポート。`${secret:...}` は `user_credentials` から当該ユーザーの値を解決して注入。
4. **API キーモードのみ** `user_credentials` テーブル + 暗号化ユーティリティ + 管理 API（`POST /api/credentials` 等、`AuthUser` スコープ）。

## ログイン UX（SPA / front/）

- 設定画面に CLI ごとの「アカウント接続」。
  - **OAuth**: `claude login` / `codex login` を実行する使い捨てターミナルセッションを開くだけ（トークンは per-user HOME に保存）。接続状態は `users/<uid>/home/.claude/` 等の存在で判定。
  - **API キー**: フォームで入力 → 暗号化して `user_credentials` に保存。
- 未接続ユーザーがエージェントを起動した場合は、CLI が出すログインプロンプト/エラーをそのまま提示。

## ライセンス / 利用規約の注意

Claude Pro/Max・ChatGPT Plus 等のサブスクリプションは**個人シート単位**であり、複数ユーザー間での共有は各社規約違反になり得る。本設計の per-user HOME モデルは「各ユーザーが自分のログインを持ち込む」ことを自然に強制するため、この観点でも適切。チーム共有 API キー（1 つの組織 API キーを全員で使う）は別の課金/規約モデルであり、`CliConfig.env` のグローバル `${env:...}` 注入で実現可能だが、利用者がコスト/規約を理解した上で admin が明示設定する運用とする。

## 触れるファイル（実装時, おおむね Phase 2〜3）

- `server-rs/bin/agent-start-host/src/sessions.rs`（`launch_env` で per-user `HOME`/`AGENT_START_*`）
- `server-rs/crates/workspace-manager/src/lib.rs`（`mark_claude_trusted` を HOME 引数化）
- `server-rs/crates/config-loader/src/config.rs` + `config.example.json`（`CliConfig.env` + 置換）
- `server-rs/crates/state/migrations/`（API キーモード時 `user_credentials`）+ `state/src/` + 暗号化ユーティリティ
- `server-rs/bin/agent-start-host/src/http/`（資格情報管理 API、`AuthUser` スコープ）
- `front/`（アカウント接続 UI）

**Exit criteria**: 2 ユーザーがそれぞれ `claude login` し、互いのサブスク/トークンを参照・上書きせずに各自のセッションでエージェントを動かせる。API キーモードでは per-user キーが暗号化保存され spawn 時に正しく注入される。

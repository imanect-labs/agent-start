# マルチユーザーのデータ / リソース・スコープ設計

親: [multiuser-rfc.md](./multiuser-rfc.md)　関連: [auth.md](./auth.md), [isolation.md](./isolation.md)

## 現状

全テーブルに `user_id` 等の列なし（`server-rs/crates/state/migrations/0001..0004`）。`sessions.name` はグローバル一意 PK。`AppState`（`app.rs`）は単一グローバルで、`PtyManager` / `ChatManager` / `sessions: Arc<RwLock<HashMap<String, SessionDirectory>>>` がユーザー文脈なしで共有される。パスは `config-loader/src/paths.rs` がすべて `~/.agent-start/` 配下に解決（各項目は env で個別 override 可、コメントに "useful for multi-user setups"）。

## スコープキー

`users.id`（TEXT、[auth.md](./auth.md)）。ユーザー所有テーブル全部に `owner_id TEXT REFERENCES users(id)` を追加。

## マイグレーション `0006_scoping.sql`

SQLite は非 NULL デフォルト付き FK 列の追加が困難なため、**nullable で追加 → アプリ起動時にバックフィル**:

```sql
ALTER TABLE sessions              ADD COLUMN owner_id TEXT;
ALTER TABLE pty_history           ADD COLUMN owner_id TEXT;
ALTER TABLE pty_snapshot          ADD COLUMN owner_id TEXT;
ALTER TABLE code_server_instances ADD COLUMN owner_id TEXT;
ALTER TABLE chat_messages         ADD COLUMN owner_id TEXT;
CREATE INDEX idx_sessions_owner       ON sessions(owner_id);
CREATE INDEX idx_chat_owner           ON chat_messages(owner_id);
CREATE INDEX idx_code_server_owner    ON code_server_instances(owner_id);
```

**セッション名スコープの決定（D2）**: 当面は **グローバル一意名 + `AND owner_id = ?` フィルタ**（変更範囲が小さく低リスク）。将来 `(owner_id, name)` 複合 PK へ移行する余地を残す。WS/プロキシの `name` ルックアップに `AND owner_id = ?` ガードを追加。

## state クレート API

`state/src/` の read/write 関数に `owner_id: &str` を追加し `WHERE owner_id = ?` / `INSERT ... owner_id` を付与する。対象（最大の機械的変更点）:
`list_all_sessions` / `mark_dead` / `mark_all_running_dead` / `save_pty_snapshot` / `load_pty_snapshot` / `clear_code_server` / chat 系（`next_chat_seq`、メッセージ insert/list）など。

## AppState / マネージャ（共有のままキーを namespace 化, D3）

マネージャはユーザー分割せず、**in-memory キーに `owner_id` を含める**:

- `PtyManager`: キー `(session_name, window)` → `(owner_id, session_name, window)`（`manager.rs` の `HashMap` キー型と呼び出し側）。
- `sessions: Arc<RwLock<HashMap<...>>>`: キーを `(owner_id, name)` 化、または `HashMap<owner_id, HashMap<name, SessionDirectory>>`。`SessionDirectory`（`sessions.rs`）に `owner_id` フィールド追加。
- 起動時 rehydration ループ（`app.rs`）と chat rehydrate、exit hooks に `owner_id` を伝播。

## ユーザー別ファイルレイアウト（`paths.rs`）

```
~/.agent-start/
  host.db                # 共有（マルチテナントテーブル）
  config.json            # システム/グローバル（CLI 定義・shell）→ admin 管理
  users/<uid>/
    home/                # 当該ユーザーのエージェントの $HOME（isolation.md）
    projects/
    worktrees/
    preferences.json     # ユーザー別 UI 設定
```

新ヘルパ: `user_home(uid)` / `user_projects_dir(uid)` / `user_worktree_root(uid)` / `user_prefs_path(uid)`。
既存の env オーバーライド設計を活かし、子プロセスへ `AGENT_START_HOME` / `AGENT_START_PROJECTS` / `AGENT_START_WORKTREE_ROOT` / `HOME` を per-user 値で注入（[isolation.md](./isolation.md) の `launch_env`）。

`config.json`（CLI 定義・shell）はグローバル/admin 管理のまま。`preferences.json` はユーザー別化（`get/put_preferences` ハンドラが `AuthUser` を取る）。

## 既存単一ユーザーデータの移行

起動時（既存 `config_loader::migrate_legacy_layout()` に倣い `migrate_to_multiuser()` を追加、マーカーファイルで一度だけ実行）:

1. マイグレーション `0005`/`0006` 適用（列/テーブルを nullable で追加）。
2. `users` が空なら seed admin を作成（env 指定または生成しログ）。
3. `UPDATE <table> SET owner_id = ? WHERE owner_id IS NULL` を seed admin id でトランザクション実行。
4. 既存 `~/.agent-start/{projects,worktrees,preferences.json}` を `users/<admin-uid>/...` へ best-effort 移動。

## Phase 2 で触れるファイル

- `server-rs/crates/state/migrations/0006_scoping.sql` + `state/src/`（`owner_id` 対応の全関数）
- `server-rs/crates/config-loader/src/paths.rs`（per-user ヘルパ）+ `migrate_to_multiuser()`
- `server-rs/bin/agent-start-host/src/app.rs`（`AppState` マネージャキー・rehydration・exit hooks）
- `server-rs/bin/agent-start-host/src/sessions.rs`（`SessionDirectory` に `owner_id`）
- `server-rs/crates/pty-manager/src/manager.rs`・`chat-manager/src/session.rs`（キーに `owner_id`）
- 全 `server-rs/bin/agent-start-host/src/http/*`（`AuthUser` を取り `owner_id` を伝播、`preferences` 別）

**Exit criteria**: 2 ユーザーがセッション/プロジェクト/worktree/チャットを**完全分離**して見え、API・WS でクロステナント漏洩がない。

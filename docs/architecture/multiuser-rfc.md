# RFC: マルチユーザー対応 agent-start（認証 / 環境隔離 / 監視）

ステータス: **DESIGN / RFC**（設計のみ。実装は段階的に別フェーズで着手）
対象: オンプレ運用、Primary Linux、macOS/Windows ではグレースフルにデグレード
最終更新: 2026-06-01

これは agent-start を**単一ユーザー前提から複数ユーザー運用へ**拡張するための包括 RFC です。
3 本柱（**認証 / 超軽量な環境隔離 / OpenTelemetry 監視**）の概要・脅威モデル・意思決定・ロードマップを記述し、各柱の詳細は以下のコンパニオン文書に分割します。

- [auth.md](./auth.md) — 認証（Forward-Auth 既定）
- [data-scoping.md](./data-scoping.md) — マルチユーザーのデータ/リソース・スコープ
- [isolation.md](./isolation.md) — 超軽量な環境隔離
- [observability.md](./observability.md) — OpenTelemetry 監視

関連: [`enterprise/README.md`](../../enterprise/README.md) は認証 / RBAC / 監査 / マルチホスト / コスト&使用量監視を planned 機能として挙げています。本 RFC はその設計の足場です。

---

## 1. 背景（現状）

調査により以下を確認:

- **認証なし**: [`SECURITY.md`](../../SECURITY.md) に「組み込み認証/認可なし、tailnet/VPN 内でのみ使う」と明記。任意の HTTP クライアントが PTY を起動し任意コマンドを実行できる。
- **単一ユーザー前提がコード全体に**:
  - 状態はすべて単一 `~/.agent-start/`（`host.db` / `config.json` / `projects/` / `worktrees/` / `preferences.json`）に保存（`server-rs/crates/config-loader/src/paths.rs`）。
  - SQLite スキーマ（`server-rs/crates/state/migrations/0001..0004`）に `user_id` 等の列が皆無。テーブル: `sessions`(name PK), `pty_history`, `pty_snapshot`, `code_server_instances`, `chat_messages`。
  - `AppState`（`server-rs/bin/agent-start-host/src/app.rs`）は単一グローバル: 共有 DB プール、`Arc<PtyManager>` / `Arc<ChatManager>` / `Arc<CodeServerManager>` / `Arc<NovncManager>`、`Arc<RwLock<HashMap<String, SessionDirectory>>>`。
- **プロセス隔離なし**: `server-rs/crates/pty-manager/src/manager.rs` が `portable-pty` で `bash -lc` を fork/exec、`chat-manager/src/session.rs` が headless `claude` を spawn。いずれもホストユーザー権限で直接実行。名前空間 / cgroup / seccomp / rlimit なし。
- **監視は最小**: `tracing` + `tracing_subscriber::fmt` のみ（`main.rs::init_tracing`）。`TraceLayer` は router に付与済み（`app.rs`）だが OTel・メトリクスなし。

## 2. ゴール / 非ゴール

**ゴール**
- 複数ユーザーがそれぞれログインして利用できる（オンプレ）。
- 各ユーザーのセッション・ファイル・プロセスを互いに分離する（超軽量に）。
- OpenTelemetry でトレース/メトリクス/ログを外部基盤へ送れる（OTLP）。未設定時は完全 no-op。

**非ゴール（今回）**
- 完全な RBAC / 監査ログ / 組織・チーム階層（`enterprise/` の将来機能）。
- 敵対的マルチテナントを Linux 以外（macOS/Windows）で完全防御すること。これらでは隔離は**論理分離のみ**になる（脅威モデル参照）。

## 3. 脅威モデル

| 環境 | 隔離 | 想定 |
| --- | --- | --- |
| Linux + systemd（推奨本番） | 専用 OS uid + cgroup v2 制限 | 半信頼ユーザーまで。FS/資源を実分離 |
| Linux（systemd 無し） | bubblewrap user namespace | 半信頼ユーザー。資源制限は近似 |
| macOS / Windows / dev | **なし（論理分離のみ）** | **信頼された小規模チーム限定**。同一ホスト uid/FS を共有 |

`isolation_mode=none` は起動時に **WARN** で明示する。いずれのモードでも本体は引き続き信頼ネットワーク（VPN/リバースプロキシ背後）配置が前提。

## 4. 3 本柱の概要

- **認証（[auth.md](./auth.md)）**: `AUTH_MODE=proxy`（既定）でリバースプロキシ/Forward-Auth が SSO を終端し信頼ヘッダを注入。代替 `AUTH_MODE=local` は SQLite ローカルアカウント（argon2id + Cookie セッション）。WebSocket/プロキシは upgrade GET の Cookie/ヘッダで認証。
- **データ/リソース・スコープ（[data-scoping.md](./data-scoping.md)）**: ユーザー所有テーブルに `owner_id` を追加、`state` クエリと in-memory マネージャのキーを namespace 化。ユーザー別 FS レイアウト `~/.agent-start/users/<uid>/`。既存単一ユーザーデータは seed admin へ移行。
- **環境隔離（[isolation.md](./isolation.md)）**: `systemd-run --uid --scope -p MemoryMax/CPUQuota/TasksMax`（専用 OS ユーザー）を Primary、bubblewrap を Fallback、none を Degraded とする `Sandbox` トレイト抽象。
- **監視（[observability.md](./observability.md)）**: OTLP エクスポートのみ。`OTEL_EXPORTER_OTLP_ENDPOINT` 設定時のみ有効化。自己ホストは `grafana/otel-lgtm` 単一コンテナを「設定一つ」の例として提示。

## 5. 意思決定ログ

| # | 決定 | 理由 | 不採用案 |
| --- | --- | --- | --- |
| D1 | 認証既定は **proxy（Forward-Auth）** | 最軽量、既存 IdP/SSO 資産を活用、本体は OIDC を持たない | 組み込み OIDC（保守重）、mTLS（証明書配布が苦痛）、bare token（ブラウザ UX 不可） |
| D2 | スコープキーは `owner_id`、セッション名は**グローバル一意 + owner フィルタ** | 変更範囲最小・低リスク | 複合 PK `(owner_id, name)`（将来移行余地として保持） |
| D3 | マネージャは共有のまま**キーを namespace 化** | per-user 分割はメモリ/ライフサイクル複雑 | マネージャの per-user シャーディング |
| D4 | 隔離は **systemd-run + 専用 OS uid** Primary | イメージ不要・実 UID 分離・実 cgroup 制限で「超軽量」 | Docker/Podman（重い）、完全自前 ns+cgroup ラッパ（再発明） |
| D5 | 監視は **OTLP のみ・未設定で no-op** | 本体を基盤非依存に保つ・差し替え自由 | Prometheus pull の埋め込み |

## 6. ロードマップ

- **Phase 0 — ドキュメント/RFC（本成果物）**: `docs/architecture/*.md`。コード変更なし。
- **Phase 1 — 認証**: `0005_auth.sql`、`auth/` モジュール、router 分割、WS/プロキシ認証、login UI。
- **Phase 2 — スキーマ/スコープ**: `0006_scoping.sql`、`owner_id` 対応の `state`、per-user パス、マネージャキー、`AuthUser` 配線、既存データ移行。
- **Phase 3 — 隔離**: 新クレート `sandbox/`、spawn 経路リファクタ、per-user env 注入、資源制限設定。
- **Phase 4 — OTel**: otel クレート、`init_observability`、span/メトリクス計装、`grafana/otel-lgtm` デプロイ手順。

各 Phase の触れるファイル・Exit criteria は各コンパニオン文書末尾を参照。

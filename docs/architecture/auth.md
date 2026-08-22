# 認証設計（Forward-Auth 既定 + ローカルアカウント代替）

親: [multiuser-rfc.md](./multiuser-rfc.md)　関連: [data-scoping.md](./data-scoping.md)

## 現状

認証は皆無（[`SECURITY.md`](../../SECURITY.md)）。`server-rs/bin/agent-start-host/src/app.rs` の `api_router` は全ルート無認証。WebSocket（`ws.rs::ws_terminal` / `ws_chat.rs::ws_chat`）はセッション名の書式検証のみ（`workspace_manager::is_valid_session_name`）。

## モード（`AUTH_MODE` 環境変数）

### `AUTH_MODE=proxy`（既定・推奨）
oauth2-proxy / Authelia / nginx `auth_request` / Apache `mod_auth_openidc` 等が **OIDC/SSO を終端**し、信頼ヘッダ（例 `X-Auth-Request-User`、`X-Remote-User`）を注入する。本体はヘッダ検証のみで OIDC を内蔵しない。

- **セキュリティ要件**:
  - 本体は **localhost のみ bind**（`AGENT_START_BIND=127.0.0.1`）。直接接続でのヘッダ偽装を防ぐ。
  - `AGENT_START_TRUSTED_PROXY`（許可元 IP/CIDR）と `AGENT_START_PROXY_USER_HEADER`（ヘッダ名、既定 `X-Auth-Request-User`）を設定可能に。
  - ヘッダ未存在 → 401。
- **JIT プロビジョニング**: ヘッダ値（ユーザー名）が `users` に無ければ初回出現時に行を作成（`password_hash` は NULL、`role='user'`）。

### `AUTH_MODE=local`（代替・外部依存なし）
SQLite にユーザーを保持（`argon2id`）、opaque トークンを Cookie セッションで管理。プロキシを立てられない最小構成向け。

将来 `AUTH_MODE=oidc`（`openidconnect` クレートで本体内蔵）は proxy で代替できるため後回し。mTLS / bare token は SPA 既定としては不採用。

## スキーマ（新マイグレーション `0005_auth.sql`）

```sql
CREATE TABLE users (
  id            TEXT PRIMARY KEY,       -- ULID/uuid。スコープキー（data-scoping.md）
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT,                   -- argon2id PHC 文字列。proxy/oidc 時は NULL
  display_name  TEXT NOT NULL DEFAULT '',
  role          TEXT NOT NULL DEFAULT 'user',  -- 'admin' | 'user'
  disabled      INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL,
  last_login_ms INTEGER
);

CREATE TABLE auth_sessions (             -- local モード時のみ使用
  token_hash    TEXT PRIMARY KEY,        -- Cookie 値の SHA-256（生値は保存しない）
  user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at_ms INTEGER NOT NULL,
  expires_at_ms INTEGER NOT NULL,
  last_seen_ms  INTEGER NOT NULL,
  user_agent    TEXT
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions(user_id);
```

Cookie: `as_session=<32 バイト乱数 base64url>`、属性 `HttpOnly; SameSite=Lax; Path=/; Secure`（Secure は TLS/プロキシ背後で有効化）。サーバは SHA-256 のみ保存。スライディング期限（`expires_at_ms` / `last_seen_ms` を更新）。

## ミドルウェア / 抽出器

新モジュール `agent-start-host/src/auth/{mod,extractor,middleware}.rs`。

```rust
// auth/mod.rs
pub struct AuthUser { pub id: String, pub username: String, pub role: String }
pub enum AuthMode { Proxy, Local }

// auth/extractor.rs — 各ハンドラ経路。req.extensions から AuthUser を読む
impl FromRequestParts<Shared> for AuthUser {
    type Rejection = Response;  // /api は 401 JSON、HTML はログインへ
    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection>;
}

// auth/middleware.rs — /api・/v1（/v1/health 除く）に from_fn_with_state で適用
pub async fn require_auth(
    State(app): State<Shared>,
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode>;
```

`require_auth` はモードに応じて Cookie（local: `auth_sessions`→`users`）または信頼ヘッダ（proxy）を解決し、`AuthUser` を `req.extensions_mut()` に挿入（抽出器の DB 二度引きを回避）。

`app.rs` の単一 `api_router` を分割:
- **public_router**: `/v1/health`、`/api/auth/login|logout|me`、SPA fallback。
- **protected_router**: その他すべて + `.layer(from_fn_with_state(state.clone(), require_auth))`。

各 protected ハンドラは引数に `AuthUser` を取り、`owner_id`（= `AuthUser.id`）を `state`/マネージャへ渡す（[data-scoping.md](./data-scoping.md)）。

新ハンドラ `src/http/auth.rs`:
- `GET /api/auth/me` → `AuthUser`（SPA のルートゲート用、両モード）。
- local のみ: `POST /api/auth/login {username,password}`（argon2 検証 → `auth_sessions` 作成 → Cookie 設定）、`POST /api/auth/logout`。
- admin（role チェック）: `POST /api/admin/users`（作成）、一覧、disable。`users` 空なら起動時に env（`AGENT_START_ADMIN_USER` / `AGENT_START_ADMIN_PASSWORD`）から seed admin を作成（生成時はログに一度だけ出力）。

## WebSocket / リバースプロキシ認証（Cookie/ヘッダ経路）

ブラウザは `new WebSocket()` に任意ヘッダを付けられないが、**同一オリジン Cookie / プロキシ注入ヘッダは upgrade GET に乗る**。よって:

- `ws.rs::ws_terminal`、`ws_chat.rs::ws_chat`、code-server/noVNC プロキシ（`/v/{name}`・`/vnc/{name}`）は `ws.on_upgrade` の**前**に `AuthUser` 抽出器で認証（upgrade GET の `FromRequestParts` で Cookie/ヘッダを読む）。失敗時は upgrade せず 401。
- 認証後、要求された `session=<name>` が `AuthUser.id` の所有か（[data-scoping.md](./data-scoping.md) の owner フィルタ）を確認してから接続。

## SPA（front/）

- ログインページ（local モード時）。起動時 `GET /api/auth/me` でユーザーを hydrate。`/api/*` の 401 でログインへリダイレクト。`credentials: 'include'`（同一オリジン）で Cookie を同送。
- proxy モードでは login UI を出さずプロキシ側ログインへ委譲（401 → プロキシのログインフローへ）。

## 必要クレート

`argon2`、`password-hash`、`axum-extra`（`cookie` feature: `CookieJar`/`PrivateCookieJar`）、`rand`、`time`。

## Phase 1 で触れるファイル

- `server-rs/crates/state/migrations/0005_auth.sql` + `state/src/`（users/auth_sessions クエリ関数）
- `server-rs/bin/agent-start-host/src/auth/{mod,extractor,middleware}.rs`（新規）
- `server-rs/bin/agent-start-host/src/http/auth.rs`（新規）
- `server-rs/bin/agent-start-host/src/app.rs`（router 分割・`require_auth`・seed admin）
- `server-rs/bin/agent-start-host/src/{ws.rs, ws_chat.rs}` と code-server/noVNC プロキシ
- `Cargo.toml`（上記クレート）
- `front/`（login ページ・me hydration・401 リダイレクト）

**Exit criteria**: 全エンドポイントが有効セッションを要求。proxy モードでヘッダ無し → 401、ヘッダ有り → 通過。local モードで admin がログインしターミナル/チャット WS まで動作。

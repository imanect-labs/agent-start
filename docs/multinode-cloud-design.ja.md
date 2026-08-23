# agent-start マルチノード / クラウド環境 設計・ロードマップ

> Status: **Phase 0〜2 実装済み（Phase 2 は一部が実環境未検証）** / 2026-08
> 実装状況は §5 のロードマップに反映。「実装済み」「自動テスト済み」「実環境で確認済み」は
> 別物として書き分けている。
> 対象バージョン: v0.2.x の単機構成 → v0.3〜v0.5 のクラスタ構成
> 関連: [ROADMAP.md](./ROADMAP.md)（単機の機能ロードマップ。本書はその上に乗る分散レイヤ） /
> [references.ja.md](./references.ja.md)（先行事例と、そこから採った判断）

## 0. 何を作るのか

Claude Code on the web / Codex cloud / Cursor background agents に相当する
**セルフホスト可能なエージェント実行クラウド**を agent-start の上に構築する。

1. **クラウド環境化** — ブラウザ / スマホから「このリポジトリにこれをやって」を投げると、
   裏でエージェントが走り、PR が上がる。走行中はターミナルにも IDE にも接続できる。
2. **マルチノード化** — 数台〜十数台の実機 / VPS / K8s ノードに agent-start を配り、
   **リソースが空いているノードで自動起動**する。ユーザはノードを意識しない。

### 0.1 ヒアリングで確定した前提

| 論点 | 決定 |
| --- | --- |
| 提供形態 | **OSS セルフホスト機能**として公開（誰でも自分のマシン群で動かせること） |
| ノード基盤 | **常設の実機 / VPS を数台〜十数台**（+ K8s クラスタ）。オートスケールは非スコープ |
| 隔離レベル | **microVM 相当**を最終形に。ただし **executor をプラグイン化**（process / docker / microvm） |
| 主要 UX | 非同期タスク→PR / 対話 PTY / IDE (code-server) / スケジュール・イベント駆動 — **全部** |
| ネットワーク | **tailnet / VPN 前提**（公開網に晒さない） |
| 認証 | **複数ユーザ（ローカルアカウント）** |
| スケジューリング | **実測負荷 + リクエスト/リミット（K8s 型の予約）** の併用 |
| ソース配置 | **ノードローカルに bare clone をキャッシュ + worktree** |
| シークレット | **中央に保管し、起動時にノードへ注入** |
| 障害時 | **セッションは失われる / タスクは再キュー**（ライブマイグレーションは非スコープ） |
| K8s | **Helm で control plane + node agent (DaemonSet)**、隔離は **RuntimeClass (Kata/gVisor)** |
| 状態ストア | **Postgres へ移行** |
| 体制・期間 | AI エージェント主体の実装、**数週間で形にする** |

### 0.2 非ゴール（今回やらないこと）

- 実行中セッションのライブマイグレーション（PTY/プロセス状態の移送）
- クラウド VM のオートスケール（起動 / 破棄のライフサイクル管理）
- マルチテナント SaaS としての課金 / 監査 / コンプライアンス
- 公開インターネットへの直接露出（tailnet / VPN 前提を維持する）
- 分散合意（raft 等）の自前実装 — 一貫性は Postgres に委譲する

---

## 1. アーキテクチャ全体像

```text
                       ┌──────────────────────── control plane ────────────────────────┐
  Browser / PWA ──tailnet──▶  agent-start-host --role control                            │
   (PC / スマホ)        │      ├─ HTTP API + 埋め込み SPA (現行 /api /v1 と互換)          │
                       │      ├─ Auth (ローカルアカウント / セッション Cookie)            │
                       │      ├─ Scheduler  (filter → score → lease)                     │
                       │      ├─ Task queue  (FOR UPDATE SKIP LOCKED)                     │
                       │      ├─ Secret store (封筒暗号化)                                │
                       │      └─ Relay      (PTY / chat / code-server の中継)             │
                       │                    ▲  Postgres (sessions, tasks, nodes, users)   │
                       └────────────────────┼──────────────────────────────────────────┘
                                            │  outbound WebSocket (node → control)
                    ┌───────────────────────┼───────────────────────┐
                    │                       │                       │
       agent-start-host --role node   --role node            --role node (K8s DaemonSet)
        (自宅 Linux 機)                (VPS)                   (K8s Node)
         ├─ Heartbeat / Metrics        ├─ ...                  ├─ ...
         ├─ Repo cache (bare + worktree)                       ├─ Executor: k8s-pod
         └─ Executor: process | docker | firecracker           │   └─ RuntimeClass: kata
             └─ claude / codex CLI (PTY or chat)               └─ hostPath repo cache
```

### 1.1 コントロールプレーンの形（推奨案）

**「1 バイナリ + `--role` 切替」を採用する。**

```bash
agent-start-host                      # = --role all  (現状と完全に同じ挙動。既存ユーザに影響ゼロ)
agent-start-host --role control       # 中央だけ (API + UI + scheduler + relay)
agent-start-host --role node --join-url https://ctl.tailnet.ts.net --join-token <tok>
```

理由:

- **OSS の導入ハードルを最小化する。** 既存の `install.sh` 一発 / 単一バイナリ配布 /
  `agent-start-host update` の資産がそのまま生きる。バイナリを 3 つに割ると
  リリースマトリクス（5 プラットフォーム × N バイナリ）と導入手順が一気に重くなる。
- **`--role all` を既定にすることで後方互換が担保できる。** 単機ユーザには何も変わらない。
  内部的には control と node が同一プロセス内に同居し、ノード間トランスポートは
  in-process チャネル（loopback transport）になるだけ。**分散コードパスを単機でも常時
  実行する**ので、「クラスタでしか壊れないバグ」が生まれにくい。
- **リーダー選出は不要。** スケジューラの排他は Postgres の advisory lock、
  キューの取り合いは `SELECT ... FOR UPDATE SKIP LOCKED` で足りる。
  raft を自前で持つ理由がない。control plane は replica を増やしても安全にでき、
  「スケジューラループを回すのは advisory lock を取れた 1 台だけ」で済む。

トレードオフとして、node ロールにも UI アセット（rust-embed した SPA）が同梱され
バイナリが太る。これは `--role node` 時に配信を無効化するだけで実害はなく、
将来 `--no-default-features` で切り落とせる（feature flag: `ui`）。

### 1.2 ノード ↔ 中央のトランスポート

tailnet 前提だが、**接続方向は node → control の outbound 一本**に固定する。

- NAT / ファイアウォール配下の自宅機をそのまま混ぜられる
- K8s の DaemonSet Pod からも同じ経路で登録できる（Pod へは外から到達できない）
- join token だけで参加でき、ノード側に inbound ポートを開けさせない

接続確立後は **1 本の WebSocket 上を多重化**して制御・データ両方を流す
（フレーム設計は §3.2）。PTY バイト列や code-server の HTTP も
この上を relay する（v1）。将来 `direct_hint`（tailnet 上の到達可能アドレス）を
交換して、ブラウザ → ノード直結にフォールバックする最適化を入れられる余地を残す。

---

## 2. コンポーネント設計

### 2.1 クレート構成（追加分）

```text
server-rs/crates/
  cluster-proto/      # ノード↔中央のフレーム定義 (serde). 双方が依存する唯一の共有型
  cluster-node/       # node ロール: 登録/heartbeat/metrics/assign 受領/executor 呼出
  cluster-control/    # control ロール: node registry / scheduler / task queue / relay
  executor/           # Executor trait + backends (process / docker / firecracker / k8s)
  metrics-probe/      # CPU/mem/load の採取 (sysinfo)。node が定期報告
  secrets/            # 封筒暗号化 (XChaCha20-Poly1305) + キー解決
  state/              # 既存。sqlx を SQLite / Postgres の両対応へ拡張
```

既存の `pty-manager` / `chat-manager` / `git-ops` / `code-server-manager` は
**node 側のライブラリ**として再利用する（書き換え不要）。`executor` の
`process` バックエンドが、現在 `http/sessions.rs` がやっている処理を包む形になる。

### 2.2 Executor 抽象

```rust
/// 1 セッション = 1 実行体。バックエンドはノードの能力に応じて選ぶ。
#[async_trait]
pub trait Executor: Send + Sync {
    /// このバックエンドが提供する隔離レベル。スケジューラのフィルタ条件になる。
    fn profile(&self) -> IsolationProfile;   // Process | Container | MicroVm

    /// 実行体を用意する（コンテナ作成 / VM 起動 / 何もしない）。
    async fn create(&self, spec: &SessionSpec) -> Result<Handle>;

    /// 実行体の中でコマンドを起動し、PTY を返す。
    async fn spawn_pty(&self, h: &Handle, cmd: &Command, size: PtySize) -> Result<PtyPair>;

    /// 実行体の中でコマンドを起動し、stdin/stdout をストリームで返す (chat モード)。
    async fn spawn_piped(&self, h: &Handle, cmd: &Command) -> Result<PipedChild>;

    /// 実行体の中／外のポートを中央へ露出する (code-server, dev server, noVNC)。
    async fn expose_port(&self, h: &Handle, port: u16) -> Result<PortForward>;

    async fn stat(&self, h: &Handle) -> Result<ResourceUsage>;
    async fn destroy(&self, h: &Handle) -> Result<()>;
}
```

`SessionSpec` には worktree パス、環境変数（注入されたシークレット）、
requests/limits、ネットワークポリシー、マウントするリポジトリキャッシュを含める。

| backend | 隔離 | 必要条件 | 位置づけ |
| --- | --- | --- | --- |
| `process` | なし | なし | 既定。現状互換。信頼できる自分のノード向け |
| `docker` | コンテナ | dockerd / podman | 環境再現性 + cgroup による requests/limits の強制 |
| `k8s-pod` | コンテナ or microVM | K8s + (RuntimeClass) | K8s プロファイル。**Kata 指定で microVM 相当** |
| `firecracker` | microVM | Linux + KVM + rootfs image | 実機プールでの最強隔離。実装コスト最大 |

> **設計上の重要な帰結:** 「microVM 隔離を最短で得る道」は Firecracker の自前実装ではなく
> **K8s + RuntimeClass=kata**。Kata Containers は Pod 単位の `runtimeClassName` 指定だけで
> microVM 隔離になる。したがってロードマップ上は `k8s-pod` を `firecracker` より先に置く。

### 2.3 スケジューラ

**2 段構え: ハードフィルタ → スコアリング。** kube-scheduler と同じ形にする。

**フィルタ（満たさないノードは候補から落とす）**

1. ノードが `Ready`（heartbeat が閾値内）
2. `allocatable_cpu - reserved_cpu >= requests.cpu` かつ mem も同様
3. タスクの要求 `isolation` をそのノードの executor が満たす
4. ラベルセレクタ一致（`gpu=true`, `arch=arm64`, `project=foo` 等）
5. 同時実行スロット `running < max_sessions`

**スコア（高い順に選ぶ）**

```text
score = 0.35 * (1 - cpu_request_ratio)        # 予約ベースの空き
      + 0.25 * (1 - ewma(cpu_util))           # 実測負荷 (EWMA で瞬間値のブレを吸収)
      + 0.20 * (1 - ewma(mem_util))
      + 0.20 * repo_cache_hit                 # そのノードに bare clone が温まっているか
```

`repo_cache_hit` を入れるのが本設計の肝。ノードローカルキャッシュ方式では
「初回だけ clone で遅い」ので、**同じプロジェクトは温まっているノードに寄せる**。
ただし寄せすぎると偏るので重みは 0.20 に留め、負荷が高ければ他ノードへ流れる。

**ラベル項は置かない。** ラベルはフィルタであり、`admit` を抜けた時点で全候補が
セレクタに等しく一致している。そこにラベル項を足しても全候補へ同じ定数が乗るだけで
順位は変わらない — 「重み表には載っているのに効かない」項になる。
（当初 0.05 を割り当てていたが、実装レビューで無効と判明したため削除した。）

**割り当ては lease（期限付き予約）で行う。**

```text
Pending ──scheduler が選択──▶ Assigned(node_id, lease_expires_at = now + 30s)
                                   │
                    node が ack ───┘──▶ Starting ──▶ Running ──▶ Succeeded / Failed
                                   │
                    lease 期限切れ ─┴──▶ Pending (attempts += 1)
```

- スケジューラループは advisory lock を取れた control plane 1 台だけが回す
- タスク取得は `SELECT ... FROM tasks WHERE status='pending' ORDER BY priority, created_at
  FOR UPDATE SKIP LOCKED LIMIT n`
- ノードの `reserved_cpu / reserved_mem` は DB 上で加算し、
  heartbeat の実測値と突き合わせて定期的に補正（reconcile ループ）

**thundering herd 対策**: 実測負荷は EWMA + 「割り当て直後 N 秒は
そのノードのスコアにペナルティ」を掛ける（新規セッションの CPU 使用が
metrics に反映されるまでのラグを埋める）。

### 2.4 ソース配布（ノードローカルキャッシュ）

```text
~/.agent-start/
  cache/<project_id>/.repo/          # bare mirror。fetch のみ、worktree の親
  worktrees/<session_id>/            # git worktree add で切る作業ツリー
```

- 初回: `git clone --mirror <clone_url> cache/<pid>/.repo`
- 2 回目以降: `git fetch --prune` → `git worktree add -b <branch> worktrees/<sid> <base>`
- セッション終了時: `git worktree remove` + （オプションで）ブランチ削除 — 現行実装と同じ
- ノードは `node_repo_cache(node_id, project_id, last_fetch_at, size_bytes)` を中央へ報告し、
  スケジューラの `repo_cache_hit` に使う
- GC: LRU で `cache/` の総容量上限（既定 20GB）を超えたら古い順に削除

大きいリポジトリでの初回コストは `--filter=blob:none` の部分クローンをオプション化して緩和する。

### 2.5 シークレット管理

**中央保管 + 起動時注入。** 平文をディスクに置かない。

```text
users ─┬─ secrets(id, user_id, kind, name, ciphertext, nonce, created_at)
       │     kind: github_token | anthropic_api_key | claude_credentials | openai_api_key | env
       └─ 封筒暗号化: XChaCha20-Poly1305
             データ鍵はレコード毎にランダム、マスタ鍵で wrap
             マスタ鍵は AGENT_START_MASTER_KEY (env) / K8s Secret / ファイル
```

- 中央 → ノードへは **セッション起動時に、そのセッションが必要とする分だけ**送る
- ノードは受け取った平文を **ディスクに書かない**（`process` executor では子プロセスの env、
  `docker`/`k8s-pod` では tmpfs マウント or env、`firecracker` では vsock 経由で VM 内へ）
- セッション終了で破棄。ノード再起動後に残らない
- 監査: `secret_access_log(session_id, secret_id, node_id, at)` を残す
- **注意**: `claude` / `codex` CLI は OAuth 済みの `~/.claude` 等を前提とすることがある。
  注入は「credentials ファイル一式を tmpfs 上の `HOME` に materialize する」形になる。
  ノードに既存ログインがある場合のフォールバックも許可する（config で opt-in）。

### 2.6 データ経路（PTY / chat / IDE）

ブラウザは常に control plane としか話さない。

```text
Browser ──WS /ws/terminal?session=S──▶ control ──stream frame (channel=C)──▶ node ──▶ PTY
        ◀──── binary frames ─────────         ◀──────────────────────────
```

- 既存の `/ws/terminal` `/ws/chat` のワイヤ形式は**変えない**（フロント改修を最小化）
- control は session_id からノードを引き、その node の WS 上に論理チャネルを開く
- スクロールバックのリングバッファは **ノード側**に置き、再接続時に control 経由でリプレイ
- `code-server` / noVNC の HTTP プロキシも同じ多重化チャネル上を通す
  （現行の `http/code_server_proxy.rs` の転送先を「ローカルポート」から「ノードチャネル」に差し替える）
- **バックプレッシャ**: チャネル毎に送信ウィンドウを持ち、詰まったら PTY read を止める
  （暴走した agent の出力で control plane のメモリを食い潰さないため）

### 2.7 タスクキュー（非同期 → PR）

Codex cloud 型 UX の中核。

```text
POST /api/tasks
{ "projectId": "...", "prompt": "...", "agent": "claude",
  "base": "main", "requests": {"cpu": 2000, "memMb": 4096},
  "isolation": "microvm", "labels": {"gpu": "false"},
  "onComplete": { "pr": true, "draft": true } }
```

1. `tasks` に `pending` で INSERT（即座に 202 を返す）
2. スケジューラが lease してノードへ assign
3. ノードが worktree を用意 → シークレット注入 → agent CLI を起動
4. 完了検知（PTY モードの終了コード。chat モードでのタスク実行は未実装 — §5 Phase 2 参照）
5. `git-ops` で commit → push → PR 作成（既存 `git-ops/github.rs` を再利用）
6. `tasks.result_pr_url` を埋めて `succeeded`

**再キュー**: ノード喪失 / lease 期限切れ / 非ゼロ終了で `attempts += 1`,
`attempts < max_attempts` なら `pending` へ戻す。**副作用（push 済み）がある場合は
再実行しない**フラグ（`side_effects_committed`）を持ち、二重 PR を防ぐ。

**スケジュール実行 / イベント駆動**（Phase 6）はこのキューへ INSERT する
別の入口として実装する（cron 式 / GitHub webhook）。実行本体は共通。

### 2.8 認証・マルチユーザ

- ローカルアカウント: `users(id, name, email, password_hash /* argon2id */, role)`
- ログイン → HttpOnly Cookie セッション。既存の code-server プロキシは Cookie 再利用（ROADMAP §4 と同じ方針）
- CLI / スクリプト用に `api_tokens(id, user_id, token_hash, scopes, expires_at)`
- 権限は最小限: `admin`（ノード管理 / 全セッション閲覧）と `member`（自分のもの）
- projects / secrets / tasks / sessions は `owner_user_id` を持つ
- **ノードの参加は join token**（`admin` が発行、TTL 付き、使い捨て or 台数制限）。
  参加後はノード毎の長期トークンを発行して置き換える（回転可能）
- **重要**: `--role all` の単機モードでは、既定で認証をバイパスできる設定を残す
  （現行ユーザの破壊的変更を避ける。`auth.mode = "none" | "local"`）

### 2.9 状態ストア（SQLite → Postgres）

- `state` クレートを `sqlx::Any` ではなく **backend 別の実装 + 共通 trait** で二重化する
  （`sqlx::Any` はマイグレーションと型で嵌りやすい）。
- 単機 `--role all` は SQLite 既定のまま、`--database-url postgres://...` で切替
- クラスタ / Helm は Postgres 必須（advisory lock と `SKIP LOCKED` を使うため）
- マイグレーションは backend 毎にディレクトリを分ける
  （`crates/state/migrations/sqlite/`, `.../postgres/`）
- スキーマは共通の論理設計を保ち、CI で両方をマイグレーション実行して検証する

### 2.10 障害モデル

| 障害 | 挙動 |
| --- | --- |
| ノードの heartbeat 断（> 30s） | `NotReady`。新規割り当て停止。既存セッションは維持を試みる |
| ノード喪失（> 120s） | セッションを `lost` に。タスクは `pending` へ再キュー（副作用フラグ考慮） |
| control plane 再起動 | ノードが自動再接続。セッションは生存（PTY はノード側にあるため） |
| Postgres 断 | control は read-only 動作に縮退。新規受付を止め、既存ストリームは維持 |
| node 側 executor 異常 | セッションを `failed`。ログを中央へ回収 |
| ネットワーク分断 | ノード側は「孤立時は自壊しない」。復帰時に状態を control へ再同期（reconcile） |

「セッションは失われる / タスクは再キュー」というヒアリング結論に従い、
**対話セッションの復旧は行わない**。UI には `lost` を明示し、
同じ worktree から「続きを開始」できる導線だけ用意する。

---

## 3. インタフェース設計

### 3.1 HTTP API（追加分）

```text
# ノード管理 (admin)
GET    /api/nodes                     # 一覧 (status, labels, capacity, usage, sessions)
GET    /api/nodes/{id}                # 詳細 + 直近メトリクス
PATCH  /api/nodes/{id}                # labels / max_sessions / cordon (新規割当停止)
DELETE /api/nodes/{id}                # 登録解除 (drain 後)
POST   /api/nodes/{id}/drain          # 既存セッションを閉じてから外す
POST   /api/join-tokens               # join token 発行 (TTL, 使用回数)

# タスク
POST   /api/tasks                     # 投入 (§2.7)
GET    /api/tasks?status=&project=    # 一覧
GET    /api/tasks/{id}                # 詳細 (割当ノード, 試行回数, PR URL)
POST   /api/tasks/{id}/cancel
POST   /api/tasks/{id}/retry

# セッション (既存を拡張)
POST   /api/sessions                  # + nodeId(任意/固定指定), requests, isolation, labels
GET    /api/sessions                  # + nodeId, ownerUserId を返す

# シークレット
GET    /api/secrets                   # 自分のもののみ (値は返さない)
PUT    /api/secrets/{name}
DELETE /api/secrets/{name}

# 認証
POST   /api/auth/login  /logout  /me
POST   /api/auth/tokens               # API token 発行
```

既存エンドポイントの**レスポンス互換は維持**する（フィールド追加のみ）。

### 3.2 ノード ↔ 中央プロトコル

1 本の WebSocket 上に、長さ付きの JSON フレーム（バイナリペイロードのみ別枠）を流す。

```jsonc
// node → control
{"t":"hello","nodeName":"gpu-01","version":"0.3.0","token":"...",
 "os":"linux","arch":"x86_64","executors":["process","docker"],
 "capacity":{"cpuMillis":16000,"memMb":65536},"labels":{"gpu":"true"}}
{"t":"heartbeat","seq":42,"metrics":{"cpuUtil":0.31,"memUtil":0.55,"load1":2.1},
 "running":["sess-a","sess-b"],"repoCache":["proj-1","proj-2"]}
{"t":"ack","assignId":"...","ok":true,"sessionId":"..."}
{"t":"event","sessionId":"...","kind":"started|exited|failed","code":0}
{"t":"stream","ch":7,"seq":123}                 // 直後のバイナリフレームがペイロード
{"t":"stream-close","ch":7,"reason":"eof"}

// control → node
{"t":"welcome","nodeId":"...","longToken":"...","heartbeatSec":10}
{"t":"assign","assignId":"...","spec":{ /* SessionSpec */ },
 "secrets":{"GITHUB_TOKEN":"...","...":"..."},"leaseSec":30}
{"t":"cancel","sessionId":"..."}
{"t":"stream-open","ch":7,"sessionId":"...","target":{"kind":"pty","window":0}}
{"t":"stream","ch":7,"seq":88}
{"t":"http","ch":9,"sessionId":"...","port":8443,"method":"GET","path":"/","headers":{}}
```

- フレーム番号でリプレイ / 順序保証、`ch` で多重化
- 再接続時は `hello` に `resumeFrom` を載せ、生存セッションを再宣言（reconcile）
- 認証は `hello` の token 検証のみ（tailnet 前提。将来 mTLS を足せる形にする）

### 3.3 データモデル（追加テーブル）

```sql
users(id, name, email, password_hash, role, created_at)
api_tokens(id, user_id, token_hash, scopes, expires_at)
secrets(id, user_id, kind, name, ciphertext, nonce, created_at)

nodes(id, name, status, version, os, arch, executors,
      capacity_cpu_millis, capacity_mem_mb,
      reserved_cpu_millis, reserved_mem_mb,
      max_sessions, labels, cordoned, last_heartbeat_at, created_at)
node_metrics(node_id, at, cpu_util, mem_util, load1, running_count)   -- 直近 N 件のみ保持
node_repo_cache(node_id, project_id, last_fetch_at, size_bytes)

projects(id, owner_user_id, name, clone_url, default_branch, created_at)

tasks(id, owner_user_id, project_id, prompt, agent, base_branch,
      status, priority, attempts, max_attempts, side_effects_committed,
      requests_cpu_millis, requests_mem_mb, isolation, label_selector,
      node_id, lease_expires_at, session_id, result_pr_url,
      created_at, started_at, finished_at, error)

sessions(... 既存 ..., node_id, owner_user_id, task_id, isolation, requests_*)
secret_access_log(session_id, secret_id, node_id, at)
```

---

## 4. Kubernetes / Helm 設計

### 4.1 チャート構成

```text
deploy/helm/agent-start/
  Chart.yaml            # dependencies: postgresql (bitnami, 任意)
  values.yaml
  templates/
    control-deployment.yaml      # --role control
    control-service.yaml
    ingress.yaml                 # 既定 disabled (tailnet 前提)
    node-daemonset.yaml          # --role node
    node-rbac.yaml               # ServiceAccount + Role (pods create/delete, 同一 ns 限定)
    join-token-secret.yaml
    master-key-secret.yaml
    session-pod-template.yaml    # ConfigMap として node に渡す Pod テンプレ
    pvc-repo-cache.yaml          # node ごとのリポジトリキャッシュ
    servicemonitor.yaml          # 任意 (Prometheus)
```

### 4.2 `values.yaml` の骨子

```yaml
controlPlane:
  replicas: 1                       # >1 でも安全 (scheduler は advisory lock で 1 本)
  image: ghcr.io/imanect-labs/agent-start:0.3.0
  auth:
    mode: local                     # none | local
  ingress:
    enabled: false                  # 既定は tailscale operator / VPN 経由を想定

postgresql:
  enabled: true                     # false なら external.url を使う
  external:
    url: ""                         # postgres://...

nodeAgent:
  enabled: true
  nodeSelector: {}
  tolerations: []
  maxSessionsPerNode: 4
  labels: {}                        # スケジューラのラベルセレクタ用
  repoCache:
    storageClass: ""
    size: 50Gi
  executor: k8s-pod                 # process | docker | k8s-pod

session:                            # k8s-pod executor が作る Pod の設定
  runtimeClassName: kata-containers  # "" なら通常 Pod、gvisor なら "gvisor"
  image: ghcr.io/imanect-labs/agent-runtime:0.3.0
  defaultRequests: { cpu: "1", memory: "2Gi" }
  defaultLimits:   { cpu: "4", memory: "8Gi" }
  serviceAccountName: agent-session
  networkPolicy:
    enabled: true                   # egress を GitHub / Anthropic API 等に絞る

secrets:
  masterKeySecretName: agent-start-master-key
```

### 4.3 K8s での隔離: DaemonSet と RuntimeClass の噛み合わせ

ヒアリングでは「node agent は DaemonSet」「隔離は RuntimeClass (Kata)」の両方が選ばれた。
`runtimeClassName` は **Pod 単位**の設定なので、DaemonSet コンテナの中で
プロセスを起こしても Kata 隔離にはならない。そこで:

> **DaemonSet の node agent は「そのノード上の executor 代理」として振る舞い、
> セッションは `nodeName` を自分に固定した別 Pod として作る。**

```text
node agent (DaemonSet, Pod A)
   └─ executor=k8s-pod ──create──▶ session Pod B
                                      nodeName: <自ノードに固定>
                                      runtimeClassName: kata-containers
                                      volumes: repo-cache PVC (同ノード)
                                      exec/attach で PTY を張り、中央へ relay
```

これにより、

- **agent-start 側のスケジューラ / キャッシュ親和性 / relay がそのまま生きる**
  （実機プールと K8s プールを同じ仕組みで扱える）
- **Kata / gVisor による microVM 相当の隔離が得られる**
- RBAC は同一 namespace の `pods`, `pods/exec`, `pods/log` に限定できる

代替として `scheduling: kubernetes` を values で選ぶと、control plane が直接 Pod を作り
**kube-scheduler に配置を委譲**するモードも用意する（自前スケジューラを使わない道）。
K8s のみで運用するユーザにはこちらが素直なので、opt-in で両方残す。

### 4.4 実機ノードとの共存

K8s クラスタと tailnet 上の実機は**同じ control plane に登録される別プールの
ノード**として扱う。ラベル（`pool=k8s` / `pool=baremetal`）で振り分けられ、
タスクは `labelSelector` と `isolation` で行き先を選べる。

---

## 5. ロードマップ

AI エージェント主体・数週間スケール。**各フェーズ単体で使える状態を保つ**ことを制約にする。
週数は目安（1 人 + エージェント並列前提）。

### Phase 0 — 内部抽象の切り出し（完了）

現状の挙動を一切変えずに、後のフェーズが乗る足場を作る。

- [x] `executor` クレート新設。`Executor` trait と `process` バックエンド
- [x] セッション起動処理を `executor` 経由に置換（node runtime 側で `launch_plan` → PTY）
- [x] `cluster-proto` クレート（フレーム型 + loopback / WS 両対応のリンク）
- [x] `metrics-probe` クレート（`sysinfo` + EWMA で CPU/mem/load を取る）
- [x] `--role` フラグ（`all` / `control` / `node`）

**受入条件**: 既存の全テスト green、UI の挙動が完全に同じ。 ✅

### Phase 1 — マルチノード最小構成（完了） ★最初の山

- [x] `cluster-control`: ノードレジストリ、join token（TTL / 使用回数、ハッシュ保存）、
      heartbeat 受領、`GET/PATCH/DELETE /api/nodes`、`POST /api/join-tokens`
- [x] `cluster-node`: 中央への outbound WS 接続（指数バックオフ再接続）、
      hello / heartbeat / metrics 送出、node identity の永続化とトークン回転
- [x] スケジューラ v1: フィルタ（Ready / cordon / スロット / requests / isolation / labels）
      + スコア（予約 + 実測負荷 + キャッシュ親和性 + warm-up ペナルティ）+ lease
- [x] relay v1: `/ws/terminal` を「ノード上の PTY」へ多重化して中継（既存ワイヤ形式のまま）
- [x] `--role all` はループバックトランスポートで control+node 同居（互換維持）
- [x] ノードローカルの bare mirror キャッシュ + worktree（`node_repo_cache` 報告つき）
- [x] UI: `/nodes` 画面、セッション行に実行ノードのバッジ
- [x] CLI: `agent-start node list / cordon / uncordon / token`

**受入条件**: 2 台のノードを登録し、セッションを連続で作ると
**空いている方に自動で分散**し、ブラウザからどちらのターミナルも同じように操作できる。
片方を停止すると `NotReady` になり、新規は残った方に行く。 ✅

実装中に見つけて直した問題（いずれも回帰テスト付き）:

| 問題 | 影響 | 対処 |
| --- | --- | --- |
| ノードが「同名のパスが存在する」だけで同一プロジェクトとみなしていた | 別マシンの `~/projects/api` が別物でも取り違える | ローカルノード以外には `local_path` を渡さない。リモートは必ず mirror 経由 |
| セッション名が秒単位のため同秒内で衝突 | worktree / ブランチ / 主キーの奪い合い | 名前に 4 文字のランダム接尾辞を追加 |
| `git worktree add` の base をブランチ名で渡していた | git の DWIM が `-b` を無視して別ブランチを作る / bare mirror で `invalid reference` | base を **コミット SHA に解決してから**渡す |
| 同一プロジェクトの並行セッションが同時に mirror を clone | 片方が他方の途中の clone を消して双方失敗 | プロジェクト単位の非同期ロックで直列化 |
| 切断済みノードのフレーム送信が成功していた | ターミナルが繋がったまま無反応になる | `connected` チェック + 切断時に writer を abort |
| `run()` 終了後もリンクの sender を保持 | ノードが再接続できない | 終了時に sender を解放 |
| `Resources::default()` が「典型的な要求量」= 非ゼロ | 予約量が初期値ぶん水増しされる | `Default` はゼロ、要求量は `default_request()` |

### Phase 2 — タスクキューと非同期→PR（実装済み・一部未検証）

- [x] `tasks` テーブル + キューイング + lease 期限切れの再キュー
- [x] `POST /api/tasks` と一覧 / 詳細 / キャンセル / リトライ
- [x] 完了フック: commit → push → PR 作成（既存 `git-ops` 再利用）、`side_effects_committed`
- [x] PTY モードの終了コード検知（chat モードは下記のとおりスコープ外に変更）
- [x] ノード喪失時のセッション `lost` 化 + タスク再キュー
- [x] UI: 「タスクを投げる」導線、タスク一覧（進行中 / 完了 / PR リンク）

**受入条件**: スマホから「このリポジトリに〜」を投げると、
リソースの空いたノードで走り、数分後に PR リンクが出る。ノードを途中で落とすと別ノードで再実行される。

**検証状況** — 「動いた」と「動くはず」を混ぜない:

| 範囲 | 状態 |
| --- | --- |
| キューの排他 / lease / 再試行 / 再起動復旧 | 自動テスト済み（`server-rs/crates/state/tests/task_queue.rs`） |
| 投入 → ノード配置 → 実行 → commit → push | **実機で確認済み**（`server-rs/crates/cluster-control/tests/task_finalize.rs` + 実ホストでの手動確認） |
| `gh pr create` による PR 作成 | **未検証**。開発環境に `gh` が無く、E2E は push までで止めている |
| chat の `codex-proto` ドライバ | **未検証**。`codex` バイナリが無く一度も喋らせていない（`experimental` 表示） |

#### 実装上の判断（設計から変えたところ）

| 論点 | 当初案 | 実装 | 理由 |
| --- | --- | --- | --- |
| キューの排他 | `SELECT … FOR UPDATE SKIP LOCKED` | 「pending のままなら UPDATE」の条件付き更新（`rows_affected == 1` が獲得） | SQLite に `SKIP LOCKED` が無い。**所有権の判定だけが等価**で、候補選択・トランザクション境界・ロック待ち・順序保証は同じではない（競合した側は次の候補へ進まず `None` を返し、次の tick に回る）。Phase 4 では backend 非依存の抽象を挟んだうえで Postgres 実装に差し替える予定（現状の `Db` は SQLite 固定で、その抽象はまだ無い） |
| タスクの実行形態 | chat / PTY の両方 | **PTY のヘッドレス実行のみ**（`claude -p '<prompt>'`） | chat セッションはトランスクリプト永続化と `--resume` がホストローカルで、ノードに配れない。ヘッドレス PTY なら既存の relay とスケジューラにそのまま乗る |
| 完了処理の実行場所 | 中央 | **ノード側**（`Finalize` / `FinalizeResult` フレームを追加） | worktree はノードにしか無い。中央で git を叩くとローカルノード以外で必ず失敗する |
| プロンプトの渡し方 | — | `CliConfig.promptArg`（claude: `-p` / codex: `exec`） | 対話起動と同じ組み立てだと REPL が入力を待ち続け、lease 切れまで固まる |

**再実行の安全性**: `side_effects_committed` は **finalize を始める前に**保守的に立てる。
push まで到達しなかった失敗でも立つが、これは意図的 — 「push したかどうか」を
後から確実に知る方法が無い以上、立て忘れて 2 つ目の PR が生えるより安全側に倒す。
以後そのタスクは自動再キューされない。手動 `retry` は attempts をリセットするが、
このフラグは残すので、自動経路は二度と再実行しない。

**未着手として残した点**:
- chat モードのタスク実行（上記のとおり Phase 5 の relay 拡張とセット）
- タスクからの `--attachment` / base ブランチの自動 refresh（単機ロードマップ Phase 3 と重複）

### Phase 3 — 認証・マルチユーザ・シークレット（1 週） ← 次

- [ ] `users` / ログイン / Cookie セッション / `api_tokens`
- [ ] `auth.mode = none | local`（単機の既定は `none` で後方互換）
- [ ] `secrets` クレート（封筒暗号化）+ CRUD API + UI
- [ ] セッション起動時のシークレット注入（ノード側はディスクに書かない）
- [ ] 所有者スコープ（projects / tasks / sessions / secrets）と admin ロール
- [ ] `secret_access_log`、join token の発行 / 失効 / 回転

**受入条件**: 2 ユーザで 1 クラスタを共有し、互いのセッション / シークレットが見えない。
ノードに `gh auth` が無い状態でも PR が作れる。

### Phase 4 — Postgres + Helm（1 週）

- [ ] `state` の backend 二重化（SQLite / Postgres）とマイグレーション分離、CI で両方検証
- [ ] scheduler の advisory lock 化、control plane の replica > 1 検証
- [ ] コンテナイメージ（control / node / agent-runtime）と GHCR への push を CI に追加
- [ ] `deploy/helm/agent-start` chart（control Deployment + node DaemonSet + PG + RBAC）
- [ ] `helm install` のドキュメント（tailscale operator 経由の到達方法を含む）

**受入条件**: `helm install agent-start ./deploy/helm/agent-start` だけで
kind / k3s クラスタ上に立ち上がり、DaemonSet ノードにセッションが分散する。

### Phase 5 — Executor 拡充: docker / k8s-pod / Kata（1〜1.5 週）

- [ ] `docker` バックエンド（cgroup で requests/limits を強制、イメージは agent-runtime）
- [ ] `k8s-pod` バックエンド（node agent が `nodeName` 固定 Pod を作る、exec で PTY）
- [ ] `session.runtimeClassName` を values から通し、**Kata / gVisor で microVM 相当の隔離**
- [ ] NetworkPolicy テンプレート（egress 制限）
- [ ] `scheduling: kubernetes` モード（kube-scheduler 委譲）を opt-in で追加
- [ ] code-server / noVNC のクロスノードプロキシ

**受入条件**: `runtimeClassName: kata-containers` で
`--dangerously-skip-permissions` の agent を走らせても、ホストのファイルシステムに触れない。
IDE がリモートノードのセッションに対して開く。

### Phase 6 — microVM (Firecracker) と自動化（以降）

- [ ] `firecracker` バックエンド（rootfs ビルド、vsock 経由の `agent-start-shim`、TAP/NAT）
- [ ] cron スケジュール実行（RRULE / cron 式 → タスク投入）
- [ ] GitHub webhook 駆動（issue ラベル / PR コメントでタスク投入）
- [ ] ノードの drain / cordon / ローリング更新、`agent-start-host update` のクラスタ対応
- [ ] Prometheus メトリクス（`/metrics`）と ServiceMonitor
- [ ] キャッシュ GC / 容量上限 / 部分クローン

---

## 6. リスクと未決事項

| # | リスク / 論点 | 影響 | 現時点の方針 |
| --- | --- | --- | --- |
| R1 | **エージェント CLI の認証がノード間で持ち運びにくい**（`claude` の OAuth は端末ローカル） | 高 | Phase 3 で「credentials 一式を tmpfs へ materialize」方式を実装し、API キー方式も併走させる。要検証 |
| R2 | Kata / gVisor は**クラスタ側の事前準備**が必要（全クラスタで使えるわけではない） | 中 | `runtimeClassName: ""` を既定にし、Kata は opt-in。ドキュメントで前提を明示 |
| R3 | Firecracker は Linux + KVM 限定で、**rootfs 管理・ネットワーク（TAP/NAT）の実装が重い** | 中 | Phase 6 に後ろ倒し。microVM 需要は先に Kata で満たす |
| R4 | relay 経由で PTY / IDE を通すと **control plane が帯域とメモリのボトルネック**になる | 中 | チャネル毎のバックプレッシャを Phase 1 で入れる。tailnet 直結フォールバックを将来オプション化 |
| R5 | **単機ユーザへの後方互換**を壊すと OSS の既存ユーザを失う | 高 | `--role all` + `auth.mode=none` + SQLite 既定を維持。CI に「単機シナリオ」E2E を残す |
| R6 | Postgres 二重化で **クエリ互換の維持が地味に重い**（現行は SQLite 前提のクエリ） | 中 | backend 別実装 + 共通 trait。CI で両方マイグレーション & スモークを実行 |
| R7 | ノードは中央から**任意コマンド実行の指示を受ける**（設計上の信頼境界） | 高 | join token を admin 発行 / TTL 付きに。SECURITY.md にクラスタの信頼モデルを追記 |
| R8 | requests/limits をユーザが盛ると**永久に Pending** になる | 低 | 最大クラスタ容量を超える要求は投入時に 400。Pending が閾値を超えたら UI で警告 |
| R9 | ライセンス境界（open core との関係） | 中 | 本設計は**全て OSS 側**に置く前提。enterprise 側に置くものが出たら別途線引き |

### 未確定（次に決めたいこと）

1. **agent-runtime イメージの中身** — どの CLI / ツールチェインを同梱するか（言語ランタイム、`gh`、ビルドツール）。ユーザ拡張の仕組み（Dockerfile の差し替え / init スクリプト）。
2. **タスク完了の判定基準** — 「エージェントが終わった」と「タスクが成功した」は別物。テスト実行の成否を条件に含めるか。
3. **PR 作成の粒度** — 1 タスク 1 PR 固定か、既存 PR への追記を許すか。
4. **UI の情報設計** — ノード / タスク / セッションの 3 概念が増えるので、現行のプロジェクト中心の画面構成をどう再編するか。
5. **メトリクスの保持期間**と `node_metrics` の肥大化対策（TimescaleDB は使わない前提でよいか）。

---

### Phase 1〜2 で意図的に残した範囲

- **chat モードのセッションはホストローカル**。transcript 永続化と `--resume` が
  ホスト側にあるため、ノード分散は relay 拡張（Phase 5）と合わせて行う。
  タスクキューはヘッドレス PTY 実行を使うので、この制約の影響を受けない。
- **リモートセッションの git / ファイル / code-server API はホストローカルのまま**。
  ワークツリーが別マシンにあるため現状は失敗する。Phase 5 の relay 拡張で対応。
- **リモートセッションの restart / 追加ウィンドウは 409 を返す**。再開はタスクキュー
  （Phase 2）で扱うのが筋。
- **認証はまだ無い**。join token はノード参加のみを守る。ユーザ認証は Phase 3。

## 7. 最初の一歩

Phase 0 と Phase 1 の前半を 1 本の PR にまとめず、次の順で刻む:

1. `executor` trait + `process` バックエンドへの置換（挙動不変のリファクタ、レビューしやすい）
2. `cluster-proto` + `--role` フラグ（骨組みだけ）
3. ノード登録 / heartbeat / `GET /api/nodes`（まだスケジューリングしない、可視化だけ）
4. スケジューラ v1 + relay v1（ここで初めて「2 台に分散」が動く）

3 の時点で「ノードが見える」ので、UI とプロトコルの手触りを早期に確認できる。

# OpenTelemetry 監視設計（OTLP 出力のみ・未設定で no-op）

親: [multiuser-rfc.md](./multiuser-rfc.md)

## 現状

`tracing` + `tracing_subscriber::fmt` のみ（`server-rs/bin/agent-start-host/src/main.rs::init_tracing`）。`AGENT_START_LOG` でフィルタ調整。`TraceLayer::new_for_http()` は router に付与済み（`app.rs`）。OTel・メトリクスなし。

## 方針

本体は **OTLP エクスポートのみ**を持ち、収集/可視化基盤には依存しない。`OTEL_EXPORTER_OTLP_ENDPOINT`（または短縮 `AGENT_START_OTLP_ENDPOINT`）が設定されている時のみ有効化し、**未設定なら完全 no-op**（fmt ログのみ）。これにより基盤は差し替え自由（D5）。

## 必要クレート

`opentelemetry`（API）、`opentelemetry_sdk`（`rt-tokio`）、`opentelemetry-otlp`、`tracing-opentelemetry`（`tracing` span → OTel ブリッジ）、`opentelemetry-semantic-conventions`。既存 `tracing-subscriber` fmt レイヤはコンソール用に残す。メトリクスは OTel metrics API + OTLP メトリクスエクスポータ（push）で統一。

## 初期化（`main.rs::init_tracing` を `init_observability` に拡張）

- `service.name=agent-start-host`・`service.version` 等の `Resource` を構築。
- **エンドポイント設定時のみ** tracer/meter プロバイダ + OTLP エクスポータを導入し、`tracing_opentelemetry::layer()` を subscriber に追加、グローバル meter プロバイダを設定。**未設定なら OTel を一切入れず fmt のみ**。
- 標準 `OTEL_*` env（endpoint / headers / service name / sampler / protocol）を尊重 + `AGENT_START_OTLP_ENDPOINT` 短縮。
- シャットダウン（`app.rs` の Ctrl-C ハンドラ）で `provider.shutdown()` フラッシュ。

## スパン

- **HTTP**: 既存 `TraceLayer::new_for_http()` がリクエスト span を生成。OTel レイヤ導入で自動エクスポート。`make_span_with` で method/route/status/latency、ハンドラ内で `user.id`（`AuthUser`）/ `session.name` を `Span::record`。
- **spawn ライフサイクル**: PTY/chat の spawn・exit span（属性 `agent.cli` / `session.name` / `owner.id` / `isolation.mode` / `pid` / `exit_code`）。exit span は spawn span にリンク。
- **chat ターン遅延**: ユーザーメッセージ → assistant 完了の span（chat 永続化経路）。

## メトリクス（OTel instruments）

| 名前 | 種別 | 取得元 |
| --- | --- | --- |
| `http.server.request.duration` | histogram | tower-http / カスタムレイヤ |
| `agent_start.sessions.active` | gauge | `PtyManager` のライブセッション数 |
| `agent_start.sessions.active.by_user` | gauge（属性 `user.id`） | per-user セッション数（公平性/濫用シグナル） |
| `agent_start.pty.bytes` | counter（in/out） | reader タスク（`manager.rs`）と `ws.rs` 入力 |
| `agent_start.agent.spawn.count` / `.exit.count` | counter（属性 cli/owner/exit_code） | spawn/exit 経路 |
| `agent_start.chat.turn.duration` | histogram | chat 経路 |
| `agent_start.resource.mem_bytes`（任意） | gauge（属性 user.id） | systemd モード時 cgroup v2 `memory.current` |

## 自己ホスト（設定一つ）

本体は OTLP しか喋らないため基盤は差し替え自由。最小構成として **`grafana/otel-lgtm` 単一コンテナ**（OTel Collector + Tempo（traces）+ Loki（logs）+ Prometheus/Mimir（metrics）+ Grafana、OTLP-in `:4317`(gRPC) / `:4318`(HTTP)）を推奨。

```yaml
# docker-compose.observability.yml
services:
  lgtm:
    image: grafana/otel-lgtm:latest
    ports:
      - "3000:3000"   # Grafana UI
      - "4317:4317"   # OTLP gRPC
      - "4318:4318"   # OTLP HTTP
```

agent-start 側は env 一つで接続:

```sh
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
# 未設定なら OTel は完全 no-op（fmt ログのみ）
```

本番では OTel Collector を前段に独立させ、Prometheus + Tempo + Loki + Grafana へファンアウトする構成へ分割できる（本体は Collector エンドポイントだけ向ければよい）。

## Phase 4 で触れるファイル

- `Cargo.toml`（上記 otel クレート）
- `server-rs/bin/agent-start-host/src/main.rs`（`init_tracing` → `init_observability`、条件付き OTel レイヤ + meter プロバイダ）
- `server-rs/bin/agent-start-host/src/app.rs`（`TraceLayer` の span enrich、Ctrl-C で flush、active-session gauge observer）
- `server-rs/crates/pty-manager/src/manager.rs`（bytes counter、spawn/exit span+counter）
- `server-rs/crates/chat-manager/src/session.rs`（turn 遅延）

**Exit criteria**: `OTEL_EXPORTER_OTLP_ENDPOINT` 設定でトレース+メトリクス+ログが LGTM 基盤に届く。未設定で silent no-op、fmt ログは不変。

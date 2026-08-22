# 参考にしている実装

agent-start の設計で「先行事例を見てから決めた」箇所の出典。**コードは借りていない**（ライセンス上の理由もあるが、そもそも構成が違う）。ここに書くのは *どの判断のときに何を見たか* であって、追随すべき仕様ではない。

---

## paseo — <https://github.com/getpaseo/paseo>

> Orchestrate multiple coding agents from desktop and mobile

agent-start と目的がかなり近い。**複数のコーディングエージェントを 1 つの常駐デーモンが束ね、デスクトップ / モバイル / CLI から叩く**という構図はそのまま重なる。

### 構成（2026-08 時点で確認した範囲）

| 項目 | paseo | agent-start |
| --- | --- | --- |
| サーバ | Node.js デーモン + WebSocket API | Rust 単一バイナリ (axum + WS) |
| クライアント | Electron / Expo (iOS・Android) / Web / CLI | ブラウザ SPA (PWA 前提)、CLI は薄い HTTP クライアント |
| 到達性 | 独自 relay（E2E 暗号化） | tailnet / VPN 前提（公開網に出さない） |
| 対応エージェント | Claude Code / Copilot / Codex / OpenCode / Pi | Claude Code / Codex（+ config で追加） |
| 実行単位 | タスク駆動（「これをやって」を投げる） | タスク駆動 + 対話セッション（PTY / チャット）の両方 |

### ここから採った判断

1. **`provider/model` を 1 つの識別子として扱う。**
   paseo は `--provider claude/opus-4.6`、SDK では `config: { provider: "codex/gpt-5.5" }` という書き方をする。
   → agent-start のチャットも **プロバイダごとにランチャーを分けず**、コンポーザ左下のドロップダウンで
   `プロバイダ / モデル` を選ぶ形にした（`chat.providers[]` + `ChatSpawnSpec.provider`）。
   ランチャーの「Chat」は 1 本だけで、どのエージェントと話すかは会話の途中でも変えられる。

2. **タスク駆動を主、対話を従にしない。**
   paseo はタスク投入が中心で、会話を延々続ける UI ではない。agent-start はどちらも要るので
   タスクキュー（Phase 2）と対話セッションを **同じ session 実体の上に載せた** —
   タスクも worktree を切って PTY で走るので、走行中のターミナルにそのまま接続できる。

3. **デーモン 1 個にクライアント複数。**
   これは元から同じ方針だが、Expo でモバイルアプリまで作っている点は「ブラウザ PWA で足りるか」を
   考え直す材料になる。現時点では PWA を維持（インストール手順を増やしたくない）。

### 採らなかった点

- **独自 relay。** agent-start は tailnet / VPN 前提を崩さない（`SECURITY.md` の信頼モデル）。
  自前で E2E 暗号化の中継網を持つのは、OSS セルフホストとしては責任範囲が重すぎる。
- **Node.js デーモン。** 単一バイナリ配布と PTY 常駐が Rust を選んだ理由なので、ここは変えない。

### 今後見に行くと良さそうなところ

- 複数エージェントを**同一タスクに並列で当てる**ときの UI（agent-start にはまだ無い）
- モバイルでの通知・バックグラウンド挙動（タスク完了 → PR の通知は Phase 6 のネタ）
- エージェントごとのアダプタ実装（`codex` のイベント正規化は agent-start 側も
  `crates/chat-manager/src/driver/` で同じ問題に当たっている）

---

## その他

- **Claude Code on the web / Codex cloud / Cursor background agents** —
  「スマホから投げて PR が返ってくる」という体験の目標地点。
  設計方針は [multinode-cloud-design.ja.md](./multinode-cloud-design.ja.md) §0 を参照。
- **kube-scheduler** — スケジューラを「ハードフィルタ → スコアリング」の 2 段構えにしたのはここから
  （同 §2.3）。
- **参考にした macOS Desktop 製品** — 単機の機能セットの下敷き。ソースは Elastic License 2.0 なので
  **借用しない**方針を [ROADMAP.md](./ROADMAP.md) §1 冒頭で明記している。

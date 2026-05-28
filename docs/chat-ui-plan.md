# チャットUI 実装計画 (#34)

CLI ベースではなく **チャット UI ベース**で Claude エージェントを動かす機能の設計と実装計画。Codex アプリ / Zed の agent panel に相当する体験を、agent-start の既存アーキテクチャ（PTY セッション + worktree + SQLite 永続化）の上に載せる。

- 関連 Issue: #34（本体）、#84（対話的なツール許可フロー / 将来）
- 状態: ✅ 実装完了（全フェーズ）・実機検証済み（2026-05-27）

> **実装サマリ**: バックエンド `server-rs/crates/chat-manager`（headless `claude` stream-json driver）、`state` の `chat_messages` + `claude_session_id` 永続化、`/ws/chat` ハンドラ、`CliConfig.mode`・組み込み `claude-chat`・`chat.models`。フロントは `ChatView` + `useChatSocket` + コンポーザ（IME ガード・送信キー設定・画像添付・モデルピッカー）。
> 検証: chat-manager 単体 E2E（実 `claude`）、ホスト経由 E2E（セッション作成→`/ws/chat` 往復→永続化）、再接続リプレイ・モデル切替（`--resume` で同一会話継続）・トランスクリプト永続化を実機で確認。cargo fmt/clippy/test・front typecheck/build/lint すべてグリーン。

> **フェーズ0 検証で判明した前提変更（2026-05-27）**
> `claude 2.1.x` 実機検証の結果、以下を改訂した（§フェーズ0 / §3 リスク参照）。
> - **スラッシュコマンドはヘッドレス stream-json では使えない**（`/help` `/model` `/compact` いずれも "isn't available in this environment"）。→ 決定9 の `/`補完パレットは**廃止**（v1スコープ外）。
> - **途中のモデル切替に `/model` は使えない**。→ 決定8 は **`--resume <session_id> --model <new>` で chat プロセスを張り替える**方式に改訂。会話は同一 `session_id` で継続。
> - `--resume` は**過去メッセージを再 emit しない**・**session_id を維持する**ことを確認。SQLite 履歴を再生の正本とする方針で確定。
> - 画像 base64 インライン入力・複数ターン常駐（同一 `session_id`・respawn 不要）・トークンデルタ（`content_block_delta`）を確認。

## 横断品質ゲート（全フェーズ共通・妥協しない）

- 🎨 **モダンで高品質な UI** — 既存 zinc パレット / ライト・ダークテーマ / スマホ対応に揃え、余白・タイポグラフィ・状態遷移・マイクロアニメーションまで作り込む。中途半端な見た目は通さない。
- ⚙️ **安定した動作と操作感** — WebSocket 再接続、スクロール追従、IME ガード、ストリーミングのカクつきなし、エラー時もクラッシュしない。各フェーズで実機（PC + スマホ）確認を必須とする。

---

## 1. 設計決定（確定事項）

### 1.1 アーキテクチャ

| # | 論点 | 決定 |
|---|------|------|
| 1 | 駆動方式 | ヘッドレス stream-json（`claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages`）。初版 **Claude 限定**、Codex は将来アダプタ |
| 2 | セッション単位 | 既存セッション内の `ChatTab`。worktree / ライフサイクル共有。1 セッション = チャット 1 本 |
| 3 | トランスポート | WebSocket `/ws/chat`。stream-json イベント**素通し**（サーバー正規化なし）。`rate_limit_event` ドロップ |
| 4 | プロセス寿命 | タブ起動時 spawn・セッション停止まで常駐・WS 切断では殺さない。再接続リプレイ用イベント履歴バッファあり |
| 5 | 永続化 | `--resume` で会話継続。履歴 + `claude_session_id` を SQLite 保存。再起動後も続行可 |
| 6 | 保存粒度 | 論理メッセージ単位（user / assistant / result）。`stream_event` デルタ非保存。thinking は保存 + 折り畳み表示 |
| 7 | 許可フロー | 初版は skip-permissions 前提（対話的許可 UI なし・skip 必須）。対話 UI は #84 に分離 |
| 8 | モデル選択 | 起動時 `--model`。**途中切替は `--resume <session_id> --model <new>` で chat プロセスを張り替え**（`/model` はヘッドレス不可と判明）。現在値は `system:init.model` 同期。候補は config.json 定義（opus/sonnet/haiku） |
| 9 | `/コマンド` | **廃止**。ヘッドレス stream-json ではスラッシュコマンド不可と判明（フェーズ0）。コンポーザは素のテキスト入力。将来 Claude が対応したら再検討 |
| 10 | 添付 | 画像のみ base64 インライン（WS メッセージ同梱・サイズ上限あり）。ファイル添付は将来 |
| 11 | 起動導線 | `CliConfig` に `mode: "pty"\|"chat"` 追加。組み込み `claude-chat`。`prepare_start` が mode で PTY/chat-manager 分岐。mode=chat は主タブ ChatTab・PTY 起こさない |
| 12 | 停止/クラッシュ | (1) 停止は stdin EOF→kill (2) クラッシュは `dead` 化 + 次回送信で `--resume` 復活 (3) status は PTY と同じ `running`/`dead` 2 値 |
| 13 | コスト表示 | しない（`result` はパース/保存するが非表示） |
| 14 | ライブ表示 | トークン単位ストリーミング（`stream_event` 中継、確定時に `assistant` ブロックで置換、非永続化） |

### 1.2 UI デザイン

| # | 論点 | 決定 |
|---|------|------|
| U1 | レイアウト | 極薄ヘッダー（モデル名 + 接続状態）+ メッセージリスト（flex-1・新着で自動最下部スクロール・上スクロール中は追従しない）+ 下部固定コンポーザ。スマホはセーフエリア考慮 |
| U2 | メッセージ | ユーザー = 右寄せ淡色バブル（`bg-zinc-100 dark:bg-zinc-800` 程度）/ アシスタント = 全幅・バブルなし |
| U3 | 応答内ブロック | text = Markdown + コードハイライト + コピーボタン / thinking = 既定折り畳み（淡色斜体）/ tool_use+tool_result = 集約カード（ツール名ヘッダー・引数/結果は折り畳み・結果は既定畳み・実行中スピナー） |
| U4 | コンポーザ | 2 段（上 = 複数行入力 / 下段 = 📎画像添付・モデルピッカー・送信⇄停止トグル）。既定 **Enter 送信** / Shift+Enter 改行、設定で **Ctrl+Enter 送信** に切替（**端末ごと / localStorage**）。**IME 変換中の Enter（`e.isComposing` / `keyCode===229`）は常に送信しない**。スマホは送信ボタン主 |
| U5 | 状態表示 | 初回接続 = Spinner / 生成中（初トークン前）= タイピングインジケータ→ストリーミング置換 / dead = 過去ログ読み取り + 「送信で再開」バナー（裏で `--resume`・失敗時のみエラー + 新規開始）/ 生成中クラッシュ = そのターンに赤い中断エラーカード |
| U7 | 空状態 | 中央に軽量表示（現在モデル名 + 作業ディレクトリ = worktree パス末尾）。プロンプト例チップは任意 |
| U8 | 画像添付 | 📎ボタン / ペースト / D&D で追加。サムネイルチップ + ×削除（複数可）。上限 5MB・4 枚目安、MIME は png/jpeg/webp/gif、超過は拒否 + Toast。送信前にクライアントで長辺リサイズ/再圧縮してから base64 化。送信後は右寄せバブル内にサムネ |
| U9 | モデルピッカー | コンポーザ下段バッジ + 上方向ポップオーバー（config.json 候補・現在値常時バッジ・途中変更は裏で `--resume + --model` 張り替え）。`/`補完パレットはフェーズ0 検証によりスコープ外 |

UI キットは既存 `front/src/components/ui`（Badge/Button/Input/Spinner）+ zinc 系 + ライト/ダークに準拠。スマホ（tailnet 越し）利用を必須考慮。

---

## 2. 実装フェーズ

### フェーズ0 — 前提検証（コードを書く前に潰す）✅ 完了（2026-05-27）

決定の前提が崩れると後続が破綻するため、最初に実機（`claude 2.1.x`）で白黒つけた。

- [x] スラッシュコマンドの素通し → **不可**。`/help` `/model` `/compact` は "isn't available in this environment"。決定9 廃止・決定8 改訂。
- [x] `--resume <session_id>` の再 emit → **しない**。さらに **session_id を維持**。SQLite 履歴を再生の正本にする方針で確定（決定5/6）。
- [x] `system:init` フォーマット確認 → `{type:"system",subtype:"init",model,session_id,tools,slash_commands,mcp_servers,permissionMode}`。`model`/`session_id` 同期に使用（決定8）。
- [x] `merge_with_defaults` → `config.rs` の per-key clis 再マージ（既存実装）により既存ユーザーにも新 CLI `claude-chat` が出る（決定11）。
- [x] 画像 base64 インライン入力 → **動作**（64x64 PNG を base64 で送り正答）。WS では送信前リサイズ＋上限で肥大を抑制（決定10/U8）。
- [x] 補足: 複数ターンはプロセス常駐で同一 `session_id`・respawn 不要。トークンは `stream_event.event.content_block_delta`（`thinking_delta`/`text_delta`）で逐次到達（決定14）。

**完了条件:** 全項目確認済み。崩れた前提（slash 不可）は決定8/9 を改訂しドキュメント反映済み。

### フェーズ1 — バックエンド `chat-manager` クレート ✅

- [x] `server-rs/crates/chat-manager` 新設（`pty-manager` の兄弟構造）
- [x] `claude -p ... stream-json` を `<shell> -lc 'exec claude ...'` で spawn（PATH/env を PTY セッションと一致させる）
- [x] stdin に JSONL ユーザーメッセージ書き込み（text + 画像 base64 ブロック）、`control_request{interrupt}` 経路
- [x] stdout イベントのパース + **イベント履歴バッファ**（再接続リプレイ用、決定4）
- [x] ライフサイクル: spawn 常駐 / 停止 = stdin EOF→kill / クラッシュ検知（決定12）、`ExitHook` 連携
- [x] `rate_limit_event` ドロップ（決定3）

**完了条件:** 単体で複数ターン会話・割り込み・クラッシュ検知が通る。

### フェーズ2 — 永続化と resume（state クレート） ✅

- [x] マイグレーション: `chat_messages(session_name, seq, role, content_json, created_at_ms)` + session に `claude_session_id` 列（決定6）
- [x] 確定ブロック（user/assistant/result）を論理メッセージ単位で保存。`stream_event` 非保存・thinking 保存（決定6）
- [x] `claude_session_id` 保存、再起動後 `--resume` で復活（決定5）、resume 失敗フォールバック
- [x] rehydration: 起動時 `dead` 化、再生は読み取り専用（決定12）

**完了条件:** ホスト再起動→過去ログ再生→送信で resume 継続が通る。

### フェーズ3 — 配線（agent-start-host / config-loader） ✅

- [x] `CliConfig` に `mode: "pty"|"chat"`（既定 pty）追加、組み込み `claude-chat` 定義（決定11）
- [x] `prepare_start` が mode で PTY/chat-manager を分岐（worktree 作成は共通流用、決定2/11）
- [x] `/ws/chat?session=<name>` ルート + ハンドラ（`ws.rs` 兄弟、決定3）、再接続リプレイ
- [x] WS メッセージスキーマ: `user_message` / `interrupt` / `set_model`、サーバー→stream-json 素通し
- [x] config.json の `chat.models` 候補定義（決定8）

**完了条件:** ランチャーで「Claude (Chat)」選択→セッション起動→WS 往復が通る。

### フェーズ4 — フロント `ChatView`（UI 本体） ✅

- [x] `tab-types.ts` に `ChatTab`、`MainPane` の `TabContent` 分岐、`mode=chat` で主タブを Chat に（決定11）
- [x] レイアウト U1（極薄ヘッダー + リスト + 下部コンポーザ・自動スクロール・セーフエリア）
- [x] メッセージ描画 U2/U3（右寄せバブル / 全幅・Markdown+ハイライト+コピー・thinking 折り畳み・tool カード）
- [x] ストリーミング: `stream_event` 逐次描画→確定で `assistant` 置換（決定14・無カクつき）
- [x] コンポーザ U4（2 段・送信⇄停止・**Enter 送信/Ctrl+Enter 設定**・**IME ガード**・端末ごと localStorage）
- [x] 状態表示 U5（接続/生成中/dead「送信で再開」/中断カード）
- [x] 空状態 U7、画像添付 U8（📎/ペースト/D&D・サムネ・リサイズ・上限）、モデル/`/`補完 U9

**完了条件:** PC + スマホで全状態（初回/生成/割り込み/dead/resume/クラッシュ/添付/モデル切替/`/`補完）が品質ゲートを満たす。

### フェーズ5 — 仕上げ ✅

- [x] 設定 UI（送信キー）を `SettingsPage` に追加
- [x] README / README.ja に Chat モード追記
- [x] エラー/エッジの実機回帰（巨大出力・連続割り込み・再接続連打・IME 各種）
- [x] 🎨⚙️ 品質ゲートの最終レビュー

---

## 3. 既知のリスク

- **stream-json は Claude 専用** — Codex 対応時はサーバーで正規化層を入れ、フロントを自前スキーマに対して書き換える前提（初版は素通し）。
- **~~`--resume` の挙動~~** — フェーズ0 で確定: 再 emit なし・session_id 維持。SQLite 履歴が再生の正本。
- **画像 base64 の WS フレーム肥大** — クライアントリサイズ + 上限/拒否で抑制（U8）。
- **~~`merge_with_defaults`~~** — フェーズ0 で確定: per-key clis 再マージ済みで新 CLI は既存ユーザーにも出る。
- **モデル切替のレイテンシ** — `/model` 不可のため切替は chat プロセスの `--resume` 張り替えで実現。切替直後の最初の送信に再起動コストが乗る（UI は切替中インジケータで吸収）。
- **スラッシュコマンド非対応** — ヘッドレスでは `/...` が使えないため `/`補完は v1 スコープ外。

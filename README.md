# agent-start

スマホから tailnet 経由で自宅 Ubuntu サーバ上の **claude code / codex CLI** セッションを起動・管理する Web ランチャー。

- リポジトリ一覧 → タップ → tmux セッション内で `claude` か `codex` を起動
- 起動済みセッションの一覧、プレビュー、停止
- 起動時に **git worktree を作成して起動** をチェック可能
- セッション停止時に紐付く worktree (と branch) も連動削除するか選択可能
- 起動フラグ (`--dangerously-skip-permissions` / `--full-auto` 等) を UI で設定・永続化
- **SSH/Web を切ってもセッションは生き続ける** (tmux daemon に委任)

## セットアップ

```bash
git clone <repo>
cd agent-start
npm install
npm run build
npm run start
```

- 既定で `0.0.0.0:3030` で待ち受ける。tailnet 経由でスマホからアクセス。
- 設定ファイル: 初回起動時に `~/.config/agent-start/config.json` を生成する。
- 起動フラグ: UI の「設定」から保存。`~/.config/agent-start/preferences.json` に永続化。
- worktree 置き場: `~/.cache/agent-start/worktrees/<session-name>/` (環境変数 `AGENT_START_WORKTREE_ROOT` で上書き可)

## 設定ファイル

`~/.config/agent-start/config.json`:

```json
{
  "roots": ["/home/shuya/dev"],
  "sessionPrefix": "cc-",
  "shell": "/bin/bash",
  "showHidden": false,
  "gitOnly": false,
  "defaultCli": "claude",
  "clis": {
    "claude": {
      "command": "claude",
      "skipPermissionsFlag": "--dangerously-skip-permissions",
      "label": "Claude Code"
    },
    "codex": {
      "command": "codex",
      "skipPermissionsFlag": "--full-auto",
      "label": "Codex CLI"
    }
  }
}
```

CLI を追加したい場合 (例: aider, opencode) は `clis` にエントリを増やすだけ。
旧 `claudeCommand` キーは自動マイグレーションされる。

## worktree モード

起動シートの「git worktree を作って起動」をチェックすると:

1. `git -C <project> worktree add -b agent-start/<session> ~/.cache/agent-start/worktrees/<session> HEAD` を実行
2. その worktree を cwd として tmux セッション起動
3. tmux の user-option (`@worktree`, `@origpath`, `@cli`) でメタ情報を保持

セッション削除時に「worktree も削除」をチェックすると:
- `git worktree remove --force` でツリー削除
- `agent-start/*` ブランチも削除

## agent-start 自身を SSH 切断後も動かす

agent-start 自身がプロセスとして生き続ける必要がある場合、別 tmux か systemd-user で起動する。

```bash
tmux new -d -s agent-start 'cd ~/dev/agent-start && npm run start'
```

agent-start が落ちても、起動済みの claude/codex セッション (`cc-*`) には影響しない (tmux daemon が独立して動くため)。

## 仕組み

```
スマホブラウザ ── tailnet ── Next.js (3030)
                              │
                              │ execFile('tmux', ...)
                              ▼
                       tmux server (daemon)
                              │
                              ├─ cc-projecta-1700... → bash -lc 'claude ...'
                              │   @cli=claude, @worktree=...
                              └─ cc-projectb-1700... → bash -lc 'codex --full-auto'
                                  @cli=codex
```

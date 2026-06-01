# 超軽量なユーザー環境隔離設計

親: [multiuser-rfc.md](./multiuser-rfc.md)　関連: [data-scoping.md](./data-scoping.md)

## 現状

プロセス隔離はゼロ。`pty-manager/src/manager.rs` が `portable-pty` で `bash -lc '<cmd>'` を fork/exec、`chat-manager/src/session.rs` が headless `claude` を spawn。いずれもホストユーザー権限・フル FS アクセスで動作。名前空間 / cgroup / seccomp / setrlimit / capability-drop なし。env は `sessions.rs::launch_env`（`AGENT_START_ROOT_PATH` / `WORKSPACE_NAME` / `WORKSPACE_PATH` / `TERM`）で注入。

## 推奨（モードマトリクス）

| モード | 仕組み | 分離強度 | 対象 |
| --- | --- | --- | --- |
| **systemd**（Primary） | `systemd-run --uid --gid --scope -p MemoryMax= -p CPUQuota= -p TasksMax=` で**専用 OS ユーザー**起動 | 実 UID による FS 分離 + cgroup v2 資源制限 | Linux + systemd（本番） |
| **bwrap**（Fallback） | bubblewrap user namespace + per-user bind-mount root + `setrlimit` 近似 | namespace 分離、資源は近似 | Linux（systemd 無し / 非特権） |
| **none**（Degraded） | 現状どおり単一ホスト uid | **論理分離のみ**（[data-scoping.md](./data-scoping.md)） | macOS/Windows/dev |

`systemd-run` を Primary とする理由（D4）: イメージ不要・実 UID 分離・実 cgroup mem/cpu/pids 制限が得られ「超軽量」要件に最適。`--scope` なので **openpty マスタは本体側が保持**したまま子だけが別 uid・transient cgroup で動き、**PTY セマンティクスは維持**される。

不採用: Docker/Podman（イメージライフサイクル・spawn 遅延で重い）、完全自前 namespace+cgroup ラッパ（systemd-run/bwrap の再発明、特権処理が危険）。

`none` は起動時に **WARN ログ**で明示（「論理分離のみ・敵対的マルチテナント不可」）。

## サンドボックス抽象（新クレート `server-rs/crates/sandbox/`）

spawn 経路を OS/モード非依存にする小さなトレイト:

```rust
pub struct SpawnRequest<'a> {
    pub owner: &'a UserContext,        // uid/gid/home/limits
    pub argv0: &'a str,                // shell
    pub args:  &'a [String],
    pub cwd:   &'a Path,
    pub env:   &'a [(String, String)],
}
pub trait Sandbox: Send + Sync {
    /// 隔離ラップ済みの portable_pty::CommandBuilder を返す
    fn wrap(&self, req: &SpawnRequest) -> portable_pty::CommandBuilder;
    fn mode(&self) -> IsolationMode;   // SystemdRun | Bwrap | None
}
```

実装: `SystemdRunSandbox` / `BwrapSandbox` / `NoopSandbox`。起動時に `which systemd-run` / `which bwrap` / `target_os` を probe し、`AGENT_START_ISOLATION=auto|systemd|bwrap|none` で上書き。

`UserContext`: 各 agent-start ユーザーを実 OS uid にマップ。プロビジョニングは**事前作成の OS ユーザープール**（`as_user0..N` を DB マップ）を推奨。任意の root セットアップスクリプトで `useradd` する。

## spawn 経路への統合

`pty-manager` を隔離非依存に保つため、**ホスト側（`http/sessions.rs` の start ハンドラ）が `Sandbox::wrap` でラップ済み `CommandBuilder` を組み立て**、`manager.rs::spawn` を「`(shell, command)` ではなくラップ済み `CommandBuilder`（またはそれを返すクロージャ）を受け取る」形にリファクタする。これにより `sandbox` クレートを `pty-manager` の依存に入れない。

`chat-manager/src/session.rs` も同様にラップ済み argv を exec（stdin/stdout の stream-json 配線は透過のまま）。

`systemd-run` の argv 例:
```
systemd-run --uid=<u> --gid=<g> --scope --quiet \
  -p MemoryMax=2G -p CPUQuota=100% -p TasksMax=512 \
  --setenv=HOME=<userhome> --working-directory=<cwd> \
  -- <shell> -lc <command>
```

## ファイルシステム隔離

- **systemd/uid モデル**: 別 uid + `HOME=users/<uid>/home`、per-user ディレクトリは当該 uid 所有 `0700`。unix 権限で他ユーザーの worktree を読めない。追加ハードニング: `-p ProtectHome=` / `-p PrivateTmp=yes` / `-p ReadWritePaths=`。
- **bwrap モデル**: `bwrap --unshare-user --unshare-pid --die-with-parent --ro-bind / / --bind users/<uid> /home/agent --chdir <cwd> ...`（per-user 書込可 root + read-only system）。
- いずれも per-user パスを env 注入（`sessions.rs::launch_env` で `HOME` / `AGENT_START_HOME` / `AGENT_START_PROJECTS` / `AGENT_START_WORKTREE_ROOT` をユーザーのサブツリーに）。
- この per-user `HOME` 割り当ては、`claude`/`codex` のサブスク資格情報（`~/.claude/`・`~/.codex/`）の分離も同時に実現する。詳細は [credentials.md](./credentials.md)。

## 資源制限（cgroup v2）

per-user 既定（`config.json` の admin セクションで上書き可）: `MemoryMax`（例 2G）、`CPUQuota`（例 100%）、`TasksMax`（例 512）、`IOWeight`。systemd-run が cgroup v2 コントローラへ直接マップ。bwrap では `setrlimit`（RLIMIT_AS / RLIMIT_NPROC）で近似（弱め・文書化）。macOS/none: 制限なし。

## Phase 3 で触れるファイル

- `server-rs/crates/sandbox/`（新規: トレイト + 3 実装 + 起動時 probe）
- `server-rs/crates/pty-manager/src/manager.rs`（`spawn` がラップ済み `CommandBuilder` を受け取る）
- `server-rs/crates/chat-manager/src/session.rs`（ラップ済み argv を exec）
- `server-rs/bin/agent-start-host/src/http/sessions.rs`（`Sandbox` + `UserContext` でラップ生成）
- `server-rs/bin/agent-start-host/src/sessions.rs`（`launch_env` に per-user `HOME`/`AGENT_START_*`）
- `config.json` admin セクション（資源制限）、OS ユーザープロビジョニング script
- 起動時 `isolation_mode` のログ / WARN

**Exit criteria**: Linux+systemd で、あるユーザーのエージェントが別 uid・cgroup mem/cpu/pids 制限下で動き他ユーザーのファイルを読めない。macOS/none は明示ログ付きでクリーンにデグレード。

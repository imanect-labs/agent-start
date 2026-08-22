//! Starting a session, independent of who asked for one.
//!
//! Two callers want this: `POST /api/sessions`, where a person is
//! waiting with a browser open, and the task queue, where nobody is
//! waiting at all. They differ only in what they do with the outcome, so
//! the decisions — which CLI, which worktree, which node, chat or PTY —
//! live here once and both paths get the same behaviour. Before this
//! existed the queue would have needed its own copy, which is exactly
//! how a scheduler ends up quietly disagreeing with the UI about what a
//! session is.

use agent_start_api::StartSessionRequest;
use cluster_control::Demand;
use cluster_proto::{AssignSpec, IsolationProfile, ProjectRef, Resources};
use std::path::{Path as StdPath, PathBuf};

use crate::app::Shared;
use crate::sessions::SessionDirectory;

/// Upper bound on the initial-prompt length we forward to the CLI. Issue
/// bodies can be long; this keeps the spawned command line well within
/// `ARG_MAX` while preserving enough context to be useful.
pub const MAX_PROMPT_CHARS: usize = 8000;

/// One request to bring a session to life.
pub struct LaunchRequest {
    pub base: StartSessionRequest,
    /// Run the agent headlessly — hand it the prompt, let it work, let
    /// it exit. Set by the task queue; a browser launch wants the
    /// interactive session instead.
    pub headless: bool,
}

impl LaunchRequest {
    pub fn interactive(base: StartSessionRequest) -> Self {
        Self {
            base,
            headless: false,
        }
    }
}

/// Why a launch did not happen. Separated by kind so the HTTP layer can
/// answer 400 / 503 / 504 without parsing a message.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// The request itself is wrong; retrying it unchanged cannot work.
    #[error("{0}")]
    BadRequest(String),
    /// No node can take the work right now.
    #[error("{0}")]
    Unavailable(String),
    /// A node accepted but never finished starting.
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    Internal(String),
}

/// A session that is now running.
pub struct Launched {
    pub name: String,
    pub command: String,
    pub cli: String,
    pub cwd: String,
    pub worktree_path: Option<String>,
    pub node_id: String,
    pub node_name: String,
}

pub async fn launch(app: &Shared, req: LaunchRequest) -> Result<Launched, LaunchError> {
    let prepared = prepare(&req)?;
    let Prepared {
        cfg,
        cli_key,
        command,
        resolved,
        create_wt,
        is_chat,
        extra,
        prompt,
        title,
    } = prepared;

    let base_name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let name = workspace_manager::session_name(&cfg.session_prefix, &base_name);

    // Chat-mode conversations still run on this host: their transcript
    // persistence and `--resume` handling are host-local (see
    // `crate::chat`). Scheduling them across nodes needs the transcript
    // to travel with them, which the relay cannot do yet.
    if is_chat {
        let (cwd, worktree_path) = maybe_create_worktree(&resolved, &name, create_wt)?;
        let _ = workspace_manager::mark_claude_trusted(&cwd);
        return start_chat_session(
            app,
            StartChatArgs {
                cfg: &cfg,
                name: &name,
                cli_key: &cli_key,
                command: &command,
                resolved: &resolved,
                cwd: &cwd,
                worktree_path: worktree_path.as_deref(),
                extra: &extra,
                prompt: prompt.as_deref(),
                title: &title,
            },
        )
        .await;
    }

    let Some(control) = app.cluster.clone() else {
        return Err(LaunchError::Unavailable(
            "this host runs no scheduler (started with --role node)".into(),
        ));
    };

    // A project reaches another node through its `origin` remote. Without
    // one it can only run where the files already are.
    let clone_url = git_ops::origin_url(&resolved);
    let local_only = clone_url.is_none();
    let project = ProjectRef {
        id: workspace_manager::project_id(&resolved),
        name: base_name.clone(),
        local_path: resolved.to_string_lossy().into_owned(),
        clone_url,
    };

    let demand = Demand {
        requests: requests_from(&req.base),
        isolation: isolation_from(&req.base)?,
        label_selector: selector_from(&req.base)?,
        project_id: project.id.clone(),
        pinned_node: req.base.node_id.clone().filter(|s| !s.is_empty()),
        local_only,
    };

    let spec = AssignSpec {
        session: name.clone(),
        cli: cli_key.clone(),
        command: command.clone(),
        shell: cfg.shell.clone(),
        project,
        create_worktree: create_wt,
        // The PTY and chat `claude` are the same binary, and either
        // needs the worktree marked trusted before it starts.
        mark_claude_trusted: cli_key == "claude",
        requests: demand.requests,
        isolation: demand.isolation,
        env: Vec::new(),
    };

    let placed = control.start_session(spec, demand).await.map_err(|e| {
        let msg = e.to_string();
        match e {
            cluster_control::StartError::NoFit(_)
            | cluster_control::StartError::Disconnected { .. } => LaunchError::Unavailable(msg),
            cluster_control::StartError::Timeout { .. } => LaunchError::Timeout(msg),
            cluster_control::StartError::Node(_) => LaunchError::Internal(msg),
        }
    })?;

    let assigned = &placed.assigned;
    tracing::info!(
        session = %name,
        node = %placed.node_name,
        local = placed.is_local,
        headless = req.headless,
        "session scheduled"
    );

    if let Err(e) = state::insert_session(
        &app.db,
        state::NewSession {
            name: &name,
            cli: &cli_key,
            cwd: &assigned.cwd,
            command: &command,
            worktree_path: &assigned.worktree_path,
            orig_path: &assigned.orig_path,
            pid: assigned.pid.map(|v| v as i64),
            title: &title,
            node_id: &placed.node_id,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "failed to persist session metadata");
    }

    app.sessions.write().insert(
        name.clone(),
        SessionDirectory {
            name: name.clone(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            cli: cli_key.clone(),
            cwd: assigned.cwd.clone(),
            worktree_path: assigned.worktree_path.clone(),
            orig_path: assigned.orig_path.clone(),
            live: true,
            history: Vec::new(),
            title,
            node_id: placed.node_id.clone(),
            node_name: if placed.is_local {
                String::new()
            } else {
                placed.node_name.clone()
            },
        },
    );

    Ok(Launched {
        name,
        command,
        cli: cli_key,
        cwd: assigned.cwd.clone(),
        worktree_path: if assigned.worktree_path.is_empty() {
            None
        } else {
            Some(assigned.worktree_path.clone())
        },
        node_id: placed.node_id,
        node_name: placed.node_name,
    })
}

/// Resource reservation for one session: what the request asked for, or
/// a modest default that lets a laptop host a few agents at once.
fn requests_from(body: &StartSessionRequest) -> Resources {
    let default = Resources::default_request();
    Resources {
        cpu_millis: body
            .cpu_millis
            .filter(|v| *v > 0)
            .unwrap_or(default.cpu_millis),
        mem_mb: body.mem_mb.filter(|v| *v > 0).unwrap_or(default.mem_mb),
    }
}

/// Parse the requested isolation, rejecting anything unrecognized.
///
/// Falling back to `process` would be the worst possible default: a
/// typo in `microvm` would silently hand the caller *no* isolation
/// while they believe they asked for the strongest.
fn isolation_from(body: &StartSessionRequest) -> Result<IsolationProfile, LaunchError> {
    match body.isolation.as_deref().map(str::trim) {
        None | Some("") | Some("process") => Ok(IsolationProfile::Process),
        Some("container") => Ok(IsolationProfile::Container),
        Some("microvm") => Ok(IsolationProfile::MicroVm),
        Some(other) => Err(LaunchError::BadRequest(format!(
            "unknown isolation `{other}` (expected `process`, `container`, or `microvm`)"
        ))),
    }
}

fn selector_from(body: &StartSessionRequest) -> Result<Vec<(String, String)>, LaunchError> {
    let Some(raw) = body.node_selector.as_ref() else {
        return Ok(Vec::new());
    };
    raw.iter()
        .map(|s| crate::cluster::parse_label(s).map_err(LaunchError::BadRequest))
        .collect()
}

/// Inputs for `start_chat_session`, grouped to keep the arg list sane.
struct StartChatArgs<'a> {
    cfg: &'a config_loader::Config,
    name: &'a str,
    cli_key: &'a str,
    command: &'a str,
    resolved: &'a StdPath,
    cwd: &'a StdPath,
    worktree_path: Option<&'a StdPath>,
    extra: &'a str,
    prompt: Option<&'a str>,
    /// Title derived from the prompt at creation; empty for prompt-less
    /// chat launches (filled in later from the first chat message).
    title: &'a str,
}

/// Spawn a headless chat conversation (#34) instead of a PTY. Mirrors the
/// PTY path: persist the session row + in-memory directory, roll back the
/// worktree if the process fails to start.
async fn start_chat_session(
    app: &Shared,
    args: StartChatArgs<'_>,
) -> Result<Launched, LaunchError> {
    let StartChatArgs {
        cfg,
        name,
        cli_key,
        command,
        resolved,
        cwd,
        worktree_path,
        extra,
        prompt,
        title,
    } = args;

    let cli_conf = cfg
        .clis
        .get(cli_key)
        .ok_or_else(|| LaunchError::BadRequest(format!("unknown cli: {cli_key}")))?;
    let provider = cfg
        .chat
        .default_provider()
        .ok_or_else(|| LaunchError::Internal("no chat provider is configured".into()))?;
    let env = crate::sessions::launch_env(resolved, name, cwd);
    let spec = chat_manager::ChatSpawnSpec {
        name: name.to_string(),
        cwd: cwd.to_path_buf(),
        shell: cfg.shell.clone(),
        // The provider owns the command now: which agent a chat talks to
        // is picked in the composer, not frozen at launch. The CLI entry
        // only decides that this is a chat at all.
        provider: provider.id.clone(),
        driver: provider.driver.clone(),
        command: provider.command.clone(),
        skip_permissions_flag: cli_conf.skip_permissions_flag.clone(),
        extra_args: extra.to_string(),
        env,
        model: provider.default_model.clone(),
        permission_mode: None,
        resume: None,
        start_seq: 0,
    };

    let session = app.chat.insert_dormant(spec);
    crate::chat::attach_persistence(app.clone(), session.clone());
    if let Err(e) = session.start().await {
        app.chat.remove(name);
        if let Some(wt) = worktree_path {
            let _ = git_ops::remove_worktree(wt, Some(resolved), true);
        }
        return Err(LaunchError::Internal(e.to_string()));
    }

    // Forward an initial prompt as the first user turn (e.g. launching
    // from a GitHub issue).
    if let Some(prompt) = prompt {
        if let Err(e) = session.send_user_message(prompt, &[]).await {
            tracing::warn!(error = %e, session = %name, "failed to send initial chat prompt");
        }
    }

    let wt_str = worktree_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let orig_str = if worktree_path.is_some() {
        resolved.to_string_lossy().into_owned()
    } else {
        String::new()
    };

    if let Err(e) = state::insert_session(
        &app.db,
        state::NewSession {
            name,
            cli: cli_key,
            cwd: &cwd.to_string_lossy(),
            command,
            worktree_path: &wt_str,
            orig_path: &orig_str,
            pid: None,
            title,
            // Chat conversations are host-local for now (see the note in
            // `launch`), so they carry the local node's id.
            node_id: &local_node_id(app),
        },
    )
    .await
    {
        tracing::warn!(error = %e, "failed to persist chat session metadata");
    }

    app.sessions.write().insert(
        name.to_string(),
        SessionDirectory {
            name: name.to_string(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            cli: cli_key.to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            worktree_path: wt_str.clone(),
            orig_path: orig_str,
            live: true,
            history: Vec::new(),
            title: title.to_string(),
            node_id: local_node_id(app),
            node_name: String::new(),
        },
    );

    Ok(Launched {
        name: name.to_string(),
        command: command.to_string(),
        cli: cli_key.to_string(),
        cwd: cwd.to_string_lossy().into_owned(),
        worktree_path: worktree_path.map(|p| p.to_string_lossy().into_owned()),
        node_id: local_node_id(app),
        node_name: String::new(),
    })
}

/// Id of the in-process node, or empty when this host runs no scheduler.
/// Sessions the host starts itself are attributed to it so the registry
/// and the session list agree on where things are running.
pub fn local_node_id(app: &Shared) -> String {
    app.cluster
        .as_ref()
        .and_then(|c| c.local_node_id())
        .unwrap_or_default()
}

/// Resolved + validated inputs needed to spawn a session.
struct Prepared {
    cfg: config_loader::Config,
    cli_key: String,
    command: String,
    resolved: PathBuf,
    create_wt: bool,
    /// True when the selected CLI runs in headless chat mode (#34).
    is_chat: bool,
    /// Sanitized extra args (without the appended prompt).
    extra: String,
    /// Initial prompt (trimmed + capped), if any.
    prompt: Option<String>,
    /// Short title derived from the prompt; empty when there is no prompt.
    title: String,
}

fn prepare(req: &LaunchRequest) -> Result<Prepared, LaunchError> {
    let body = &req.base;
    if body.project_path.is_empty() {
        return Err(LaunchError::BadRequest("projectPath is required".into()));
    }
    let cfg = config_loader::load_config().map_err(|e| LaunchError::Internal(e.to_string()))?;
    let prefs =
        config_loader::load_preferences().map_err(|e| LaunchError::Internal(e.to_string()))?;

    let resolved = PathBuf::from(&body.project_path);
    if !config_loader::is_path_under_roots(&cfg, &resolved) {
        return Err(LaunchError::BadRequest(
            "projectPath is outside configured roots".into(),
        ));
    }

    let cli_key = body.cli.clone().unwrap_or_else(|| {
        if !prefs.cli.is_empty() {
            prefs.cli.clone()
        } else {
            cfg.default_cli.clone()
        }
    });
    let cli_conf = cfg
        .clis
        .get(&cli_key)
        .ok_or_else(|| LaunchError::BadRequest(format!("unknown cli: {cli_key}")))?;

    let skip = body.skip_permissions.unwrap_or(prefs.skip_permissions);
    let extra_raw = body
        .extra_args
        .clone()
        .unwrap_or_else(|| prefs.extra_args.clone());
    let extra = config_loader::sanitize_extra_args(&extra_raw)
        .map_err(|e| LaunchError::BadRequest(e.to_string()))?;

    let create_wt = body.create_worktree.unwrap_or(false);
    if create_wt && !git_ops::is_git_repo(&resolved) {
        return Err(LaunchError::BadRequest(
            "createWorktree requested but project is not a git repository".into(),
        ));
    }

    let is_chat = cli_conf.is_chat();

    // Capped initial prompt, shared by both launch paths.
    let prompt = body.prompt.as_deref().and_then(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(MAX_PROMPT_CHARS).collect::<String>())
        }
    });

    // Derive a sidebar title from the prompt up front. Prompt-less launches
    // (e.g. a plain chat session) get an empty title here; chat-mode fills it
    // in from the first user message instead.
    let title = prompt
        .as_deref()
        .map(crate::sessions::summarize_title)
        .unwrap_or_default();

    if req.headless && is_chat {
        return Err(LaunchError::BadRequest(
            "chat-mode CLIs cannot run a queued task; pick a terminal agent".into(),
        ));
    }
    if req.headless && prompt.is_none() {
        return Err(LaunchError::BadRequest(
            "a headless run needs a prompt to work from".into(),
        ));
    }

    // Chat mode never builds a PTY command line; it spawns the headless
    // process from its components in `start_chat_session`. The stored
    // `command` is just a human-readable descriptor.
    let command = if is_chat {
        format!("{} (chat)", cli_conf.command)
    } else if req.headless {
        // The prompt is the whole job: the agent must take it, work, and
        // exit rather than open a REPL nobody is sitting at.
        config_loader::build_headless_command(
            cli_conf,
            skip,
            &extra,
            prompt.as_deref().unwrap_or_default(),
        )
        .map_err(|e| LaunchError::BadRequest(e.to_string()))?
    } else {
        let mut command = config_loader::build_launch_command(cli_conf, skip, &extra)
            .map_err(|e| LaunchError::BadRequest(e.to_string()))?;
        // An initial prompt (e.g. launching from a GitHub issue) is handed
        // to the agent CLI as a positional argument. Skip it for the
        // bare-shell CLI (empty command) which has no prompt argument.
        if let Some(prompt) = &prompt {
            if !command.is_empty() {
                command.push(' ');
                command.push_str(&config_loader::shell_quote(prompt));
            }
        }
        command
    };

    Ok(Prepared {
        cfg,
        cli_key,
        command,
        resolved,
        create_wt,
        is_chat,
        extra,
        prompt,
        title,
    })
}

fn maybe_create_worktree(
    orig_path: &StdPath,
    session_name: &str,
    create: bool,
) -> Result<(PathBuf, Option<PathBuf>), LaunchError> {
    if !create {
        return Ok((orig_path.to_path_buf(), None));
    }
    match git_ops::create_worktree(orig_path, session_name) {
        Ok(wt) => Ok((wt.worktree_path.clone(), Some(wt.worktree_path))),
        Err(e) => Err(LaunchError::Internal(format!(
            "worktree creation failed: {e}"
        ))),
    }
}

//! The zero-isolation backend: run the agent as a plain child of the
//! node process, exactly as agent-start did before the cluster layer.
//!
//! It is the default so a single-host install behaves identically to
//! v0.2, and it is the reference implementation of the trait: every
//! other backend differs only in `create`/`launch_plan`/`destroy`.

use crate::{ExecError, Executor, Handle, LaunchPlan, SessionSpec};
use async_trait::async_trait;
use cluster_proto::IsolationProfile;

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessExecutor;

#[async_trait]
impl Executor for ProcessExecutor {
    fn name(&self) -> &'static str {
        "process"
    }

    fn profile(&self) -> IsolationProfile {
        IsolationProfile::Process
    }

    async fn create(&self, spec: &SessionSpec) -> Result<Handle, ExecError> {
        // Nothing to build — but fail here rather than at PTY spawn time
        // if the working directory is gone, so the control plane gets a
        // useful error instead of a shell that dies immediately.
        if !spec.cwd.is_dir() {
            return Err(ExecError::Backend(format!(
                "working directory does not exist: {}",
                spec.cwd.display()
            )));
        }
        Ok(Handle {
            id: spec.session.clone(),
            backend: "process",
        })
    }

    fn launch_plan(&self, _handle: &Handle, spec: &SessionSpec) -> LaunchPlan {
        LaunchPlan {
            shell: spec.shell.clone(),
            command: spec.command.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
        }
    }

    async fn destroy(&self, _handle: &Handle) -> Result<(), ExecError> {
        // The PTY child is killed by `pty-manager`; there is no sandbox
        // left over to reclaim.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_proto::Resources;
    use std::path::PathBuf;

    fn spec(cwd: PathBuf) -> SessionSpec {
        SessionSpec {
            session: "cc-demo-1".into(),
            cwd,
            shell: "/bin/bash".into(),
            command: "claude".into(),
            env: vec![("FOO".into(), "bar".into())],
            requests: Resources::default_request(),
        }
    }

    #[tokio::test]
    async fn create_rejects_a_missing_working_directory() {
        let ex = ProcessExecutor;
        let err = ex
            .create(&spec(PathBuf::from("/nonexistent/agent-start/xyz")))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn launch_plan_passes_the_command_through_untouched() {
        let ex = ProcessExecutor;
        let s = spec(std::env::temp_dir());
        let handle = ex.create(&s).await.unwrap();
        let plan = ex.launch_plan(&handle, &s);
        assert_eq!(plan.command, "claude");
        assert_eq!(plan.shell, "/bin/bash");
        assert_eq!(plan.env, s.env);
    }
}

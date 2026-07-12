use std::ffi::OsString;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{spawn_contained, ContainedProcess, ContainedProcessControl};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessStopReason {
    Exited,
    TimedOut,
    Cancelled,
    DaemonShutdown,
    ContainmentFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProcessContainmentEvidence {
    pub method: String,
    pub root_pid: Option<u32>,
    pub stop_reason: ProcessStopReason,
    pub active_processes_after_cleanup: Option<u32>,
    pub descendants_terminated: Option<bool>,
    pub containment_confirmed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct ContainedCommandSpec {
    pub executable: OsString,
    pub args: Vec<OsString>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl ContainedCommandSpec {
    #[must_use]
    pub fn new(executable: impl Into<OsString>) -> Self {
        Self {
            executable: executable.into(),
            args: Vec::new(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(30),
            max_stdout_bytes: 64 * 1024,
            max_stderr_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainedProcessOutput {
    pub execution_id: Uuid,
    pub root_pid: u32,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub containment: ProcessContainmentEvidence,
}

#[derive(Debug, Error)]
pub enum ContainmentError {
    #[error("contained execution is supported only on Windows")]
    UnsupportedPlatform,
    #[error("Windows process containment is unavailable: {0}")]
    ContainmentUnavailable(String),
    #[error("failed to start contained process: {0}")]
    SpawnFailed(String),
    #[error("failed while waiting for contained process: {0}")]
    WaitFailed(String),
    #[error("failed to request contained process termination: {0}")]
    TerminationFailed(String),
    #[error("contained process completion channel closed")]
    CompletionChannelClosed,
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Default)]
pub struct ContainedProcessControl;

#[cfg(not(windows))]
impl ContainedProcessControl {
    pub fn cancel(&self, _reason: ProcessStopReason) -> Result<(), ContainmentError> {
        Err(ContainmentError::UnsupportedPlatform)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct ContainedProcess;

#[cfg(not(windows))]
impl ContainedProcess {
    #[must_use]
    pub fn execution_id(&self) -> Uuid {
        Uuid::nil()
    }

    #[must_use]
    pub fn root_pid(&self) -> u32 {
        0
    }

    #[must_use]
    pub fn control(&self) -> ContainedProcessControl {
        ContainedProcessControl
    }

    pub async fn wait(self) -> Result<ContainedProcessOutput, ContainmentError> {
        Err(ContainmentError::UnsupportedPlatform)
    }
}

#[cfg(not(windows))]
pub fn spawn_contained(_spec: ContainedCommandSpec) -> Result<ContainedProcess, ContainmentError> {
    Err(ContainmentError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_spec_has_bounded_defaults() {
        let spec = ContainedCommandSpec::new("fixture");
        assert_eq!(spec.timeout, Duration::from_secs(30));
        assert_eq!(spec.max_stdout_bytes, 64 * 1024);
        assert_eq!(spec.max_stderr_bytes, 64 * 1024);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_mutating_execution_fails_closed() {
        assert!(matches!(
            spawn_contained(ContainedCommandSpec::new("fixture")),
            Err(ContainmentError::UnsupportedPlatform)
        ));
    }
}

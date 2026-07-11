use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::ContainedChild;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::ContainedChild;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub descendant_grace: Duration,
    pub termination_grace: Duration,
    pub max_capture_bytes: usize,
}

impl ProcessSpec {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>, args: Vec<OsString>) -> Self {
        Self {
            executable: executable.into(),
            args,
            stdin: Vec::new(),
            timeout: Duration::from_secs(30),
            descendant_grace: Duration::from_secs(5),
            termination_grace: Duration::from_secs(5),
            max_capture_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CancellationReason {
    ExplicitCancellation = 1,
    DaemonShutdown = 2,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    state: Arc<AtomicU8>,
}

impl CancellationToken {
    pub fn cancel(&self, reason: CancellationReason) -> bool {
        self.state
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    #[must_use]
    pub fn reason(&self) -> Option<CancellationReason> {
        match self.state.load(Ordering::Acquire) {
            1 => Some(CancellationReason::ExplicitCancellation),
            2 => Some(CancellationReason::DaemonShutdown),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.reason().is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    ProcessExited,
    TimedOut,
    ExplicitCancellation,
    DaemonShutdown,
    DescendantsOutlivedRoot,
    ContainmentFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainmentReport {
    pub method: String,
    pub root_process_id: u32,
    pub kill_on_job_close: bool,
    pub assigned_at_creation: bool,
    pub inherited_handle_list_restricted: bool,
    pub termination_reason: TerminationReason,
    pub termination_requested: bool,
    pub termination_confirmed: bool,
    pub active_processes_after: Option<u32>,
    pub descendants_outlived_root: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainedOutput {
    pub exit_code: Option<u32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration_ms: u64,
    pub containment: ContainmentReport,
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("Windows Job Object containment is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("{operation} failed: {source}")]
    WindowsApi {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid process specification: {0}")]
    InvalidSpecification(String),
    #[error("process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("process output reader thread failed")]
    ReaderThreadFailed,
}

pub type Result<T> = std::result::Result<T, JobError>;

pub(crate) fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

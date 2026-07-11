use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessTerminationReason {
    ProcessExited,
    TimedOut,
    ExplicitCancellation,
    DaemonShutdown,
    DescendantsOutlivedRoot,
    ContainmentFailure,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProcessContainmentEvidence {
    pub method: String,
    pub root_process_id: Option<u32>,
    pub kill_on_job_close: bool,
    pub assigned_at_creation: bool,
    pub inherited_handle_list_restricted: bool,
    pub termination_reason: ProcessTerminationReason,
    pub termination_requested: bool,
    pub termination_confirmed: bool,
    pub active_processes_after: Option<u32>,
    pub descendants_outlived_root: bool,
    pub detail: Option<String>,
}

impl ProcessContainmentEvidence {
    #[must_use]
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            method: "UNAVAILABLE".to_owned(),
            root_process_id: None,
            kill_on_job_close: false,
            assigned_at_creation: false,
            inherited_handle_list_restricted: false,
            termination_reason: ProcessTerminationReason::Unavailable,
            termination_requested: false,
            termination_confirmed: false,
            active_processes_after: None,
            descendants_outlived_root: false,
            detail: Some(detail.into()),
        }
    }

    #[must_use]
    pub fn process_tree_terminated(&self) -> bool {
        self.termination_confirmed && self.active_processes_after == Some(0)
    }
}

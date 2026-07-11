use crate::{CancellationToken, ContainedOutput, JobError, ProcessSpec, Result};

#[derive(Debug)]
pub struct ContainedChild;

impl ContainedChild {
    pub fn spawn(_spec: ProcessSpec) -> Result<Self> {
        Err(JobError::UnsupportedPlatform)
    }

    pub fn wait(self, _cancellation: &CancellationToken) -> Result<ContainedOutput> {
        Err(JobError::UnsupportedPlatform)
    }
}

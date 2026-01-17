use std::error::Error;
use std::fmt;

use bunch_core::ArtifactManifest;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Queued,
    Preparing,
    Running,
    Finalizing,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Queued,
                Self::Preparing | Self::Cancelling | Self::Failed
            ) | (
                Self::Preparing,
                Self::Running | Self::Cancelling | Self::Failed
            ) | (
                Self::Running,
                Self::Finalizing | Self::Cancelling | Self::Failed
            ) | (
                Self::Finalizing,
                Self::Succeeded | Self::Cancelling | Self::Failed
            ) | (Self::Cancelling, Self::Cancelled | Self::Failed)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum JobEventKind {
    StateChanged {
        from: Option<JobState>,
        to: JobState,
    },
    Progress {
        completed: u32,
        total: u32,
        message: String,
    },
    ArtifactProduced {
        artifact: ArtifactManifest,
    },
    Diagnostic {
        level: DiagnosticLevel,
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobEvent {
    pub protocol_version: u32,
    pub job_id: JobId,
    pub sequence: u64,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub event: JobEventKind,
}

impl JobEvent {
    #[must_use]
    pub fn terminal_state(&self) -> Option<JobState> {
        match self.event {
            JobEventKind::StateChanged { to, .. } if to.is_terminal() => Some(to),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub job_id: JobId,
    pub state: Option<JobState>,
    pub progress_completed: u32,
    pub progress_total: u32,
    pub progress_message: String,
    pub artifacts: Vec<ArtifactManifest>,
    pub diagnostics: Vec<String>,
    pub last_sequence: u64,
}

impl JobSnapshot {
    #[must_use]
    pub fn new(job_id: JobId) -> Self {
        Self {
            job_id,
            state: None,
            progress_completed: 0,
            progress_total: 100,
            progress_message: "Waiting".to_owned(),
            artifacts: Vec::new(),
            diagnostics: Vec::new(),
            last_sequence: 0,
        }
    }

    pub fn apply(&mut self, event: &JobEvent) -> Result<(), ProtocolError> {
        if event.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::Version(event.protocol_version));
        }
        if event.job_id != self.job_id {
            return Err(ProtocolError::JobMismatch);
        }
        if event.sequence <= self.last_sequence {
            return Err(ProtocolError::StaleSequence {
                received: event.sequence,
                last: self.last_sequence,
            });
        }

        match &event.event {
            JobEventKind::StateChanged { from, to } => match self.state {
                None if from.is_none() && *to == JobState::Queued => self.state = Some(*to),
                Some(current) if *from == Some(current) && current.can_transition_to(*to) => {
                    self.state = Some(*to);
                }
                current => {
                    return Err(ProtocolError::InvalidTransition {
                        current,
                        declared_from: *from,
                        next: *to,
                    });
                }
            },
            JobEventKind::Progress {
                completed,
                total,
                message,
            } => {
                if *total == 0 || completed > total {
                    return Err(ProtocolError::InvalidProgress {
                        completed: *completed,
                        total: *total,
                    });
                }
                self.progress_completed = *completed;
                self.progress_total = *total;
                self.progress_message.clone_from(message);
            }
            JobEventKind::ArtifactProduced { artifact } => {
                self.artifacts.push(artifact.clone());
            }
            JobEventKind::Diagnostic { message, .. } => {
                self.diagnostics.push(message.clone());
            }
        }
        self.last_sequence = event.sequence;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    Version(u32),
    JobMismatch,
    StaleSequence {
        received: u64,
        last: u64,
    },
    InvalidTransition {
        current: Option<JobState>,
        declared_from: Option<JobState>,
        next: JobState,
    },
    InvalidProgress {
        completed: u32,
        total: u32,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_event(sequence: u64, from: Option<JobState>, to: JobState) -> JobEvent {
        JobEvent {
            protocol_version: PROTOCOL_VERSION,
            job_id: JobId("job-1".to_owned()),
            sequence,
            elapsed_ms: sequence,
            step_id: None,
            event: JobEventKind::StateChanged { from, to },
        }
    }

    #[test]
    fn valid_lifecycle_reaches_success() {
        let mut snapshot = JobSnapshot::new(JobId("job-1".to_owned()));
        let states = [
            (None, JobState::Queued),
            (Some(JobState::Queued), JobState::Preparing),
            (Some(JobState::Preparing), JobState::Running),
            (Some(JobState::Running), JobState::Finalizing),
            (Some(JobState::Finalizing), JobState::Succeeded),
        ];
        for (index, (from, to)) in states.into_iter().enumerate() {
            snapshot
                .apply(&state_event((index + 1) as u64, from, to))
                .expect("valid transition");
        }
        assert_eq!(snapshot.state, Some(JobState::Succeeded));
    }

    #[test]
    fn stale_and_invalid_transitions_are_rejected() {
        let mut snapshot = JobSnapshot::new(JobId("job-1".to_owned()));
        snapshot
            .apply(&state_event(1, None, JobState::Queued))
            .expect("queued");
        assert!(matches!(
            snapshot.apply(&state_event(1, Some(JobState::Queued), JobState::Preparing)),
            Err(ProtocolError::StaleSequence { .. })
        ));
        assert!(matches!(
            snapshot.apply(&state_event(2, Some(JobState::Queued), JobState::Succeeded)),
            Err(ProtocolError::InvalidTransition { .. })
        ));
    }
}

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use bunch_core::{ArtifactManifest, Operation};
use bunch_planner::ExecutionPlan;
use bunch_protocol::{DiagnosticLevel, JobEvent, JobEventKind, JobId, JobState, PROTOCOL_VERSION};

use crate::CancellationToken;
use crate::capabilities::FOUNDATION_ADAPTER_ID;

#[derive(Clone, Debug)]
pub struct NativeRunner {
    next_id: Arc<AtomicU64>,
}

impl Default for NativeRunner {
    fn default() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl NativeRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&self, plan: ExecutionPlan) -> Result<JobHandle, RunnerError> {
        if plan.steps.is_empty()
            || plan
                .steps
                .iter()
                .any(|step| step.capability.adapter.id != FOUNDATION_ADAPTER_ID)
        {
            return Err(RunnerError::UnsupportedPlan);
        }

        let numeric_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = JobId(format!("job-{numeric_id:06}"));
        let cancellation = CancellationToken::new();
        let thread_cancellation = cancellation.clone();
        let thread_id = id.clone();
        let (events, receiver) = async_channel::bounded(64);
        thread::Builder::new()
            .name(format!("bunch-{numeric_id:06}"))
            .spawn(move || run_foundation_job(thread_id, plan, thread_cancellation, events))
            .map_err(|error| RunnerError::Thread(error.to_string()))?;

        Ok(JobHandle {
            id,
            events: receiver,
            cancellation,
        })
    }
}

#[derive(Debug)]
pub struct JobHandle {
    id: JobId,
    events: Receiver<JobEvent>,
    cancellation: CancellationToken,
}

impl JobHandle {
    #[must_use]
    pub fn id(&self) -> &JobId {
        &self.id
    }

    #[must_use]
    pub fn events(&self) -> Receiver<JobEvent> {
        self.events.clone()
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerError {
    UnsupportedPlan,
    Thread(String),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlan => write!(formatter, "plan is not supported by the M1 runner"),
            Self::Thread(error) => write!(formatter, "could not start job thread: {error}"),
        }
    }
}

impl Error for RunnerError {}

fn run_foundation_job(
    id: JobId,
    plan: ExecutionPlan,
    cancellation: CancellationToken,
    sender: Sender<JobEvent>,
) {
    let mut emitter = Emitter::new(id, sender);
    if !emitter.change_state(JobState::Queued) {
        return;
    }
    if cancel_if_requested(&mut emitter, &cancellation) {
        return;
    }
    if !emitter.change_state(JobState::Preparing) {
        return;
    }
    if cancel_if_requested(&mut emitter, &cancellation) {
        return;
    }
    if !emitter.change_state(JobState::Running) {
        return;
    }

    let step_count = plan.steps.len() as u32;
    for (step_index, step) in plan.steps.iter().enumerate() {
        match &step.operation {
            Operation::FoundationSimulate(parameters) => {
                let slice = Duration::from_millis((parameters.duration_ms / 10).max(1));
                for percent in (0_u32..=100).step_by(10) {
                    if cancel_if_requested(&mut emitter, &cancellation) {
                        return;
                    }
                    if parameters
                        .fail_at_percent
                        .is_some_and(|failure| percent >= u32::from(failure))
                    {
                        emitter.diagnostic(
                            DiagnosticLevel::Error,
                            "foundation-requested-failure",
                            "The foundation runner reached its configured failure point.",
                            Some(&step.id),
                        );
                        emitter.change_state(JobState::Failed);
                        return;
                    }
                    let completed = ((step_index as u32 * 100) + percent) / step_count;
                    if !emitter.progress(
                        completed.min(100),
                        100,
                        &parameters.output_label,
                        &step.id,
                    ) {
                        return;
                    }
                    if percent < 100 {
                        thread::sleep(slice);
                    }
                }
            }
        }
    }

    if cancel_if_requested(&mut emitter, &cancellation) {
        return;
    }
    if !emitter.change_state(JobState::Finalizing) {
        return;
    }
    let adapter = &plan.steps[0].capability.adapter;
    if !emitter.artifact(ArtifactManifest {
        name: plan.output_name.clone(),
        locator: format!("fake://{}", plan.output_name),
        media_type: "application/x-bunch-foundation".to_owned(),
        byte_length: 0,
        adapter_id: adapter.id.clone(),
        engine_name: adapter.engine_name.clone(),
        engine_version: adapter.engine_version.clone(),
        warnings: vec!["Foundation artifact only; no file was written".to_owned()],
    }) {
        return;
    }
    emitter.change_state(JobState::Succeeded);
}

fn cancel_if_requested(emitter: &mut Emitter, cancellation: &CancellationToken) -> bool {
    if !cancellation.is_cancelled() {
        return false;
    }
    if !matches!(emitter.state, Some(JobState::Cancelling)) {
        emitter.change_state(JobState::Cancelling);
    }
    emitter.change_state(JobState::Cancelled);
    true
}

struct Emitter {
    id: JobId,
    sender: Sender<JobEvent>,
    sequence: u64,
    started: Instant,
    state: Option<JobState>,
}

impl Emitter {
    fn new(id: JobId, sender: Sender<JobEvent>) -> Self {
        Self {
            id,
            sender,
            sequence: 0,
            started: Instant::now(),
            state: None,
        }
    }

    fn change_state(&mut self, next: JobState) -> bool {
        let previous = self.state;
        let sent = self.send(
            None,
            JobEventKind::StateChanged {
                from: previous,
                to: next,
            },
        );
        if sent {
            self.state = Some(next);
        }
        sent
    }

    fn progress(&mut self, completed: u32, total: u32, message: &str, step_id: &str) -> bool {
        self.send(
            Some(step_id.to_owned()),
            JobEventKind::Progress {
                completed,
                total,
                message: message.to_owned(),
            },
        )
    }

    fn artifact(&mut self, artifact: ArtifactManifest) -> bool {
        self.send(None, JobEventKind::ArtifactProduced { artifact })
    }

    fn diagnostic(
        &mut self,
        level: DiagnosticLevel,
        code: &str,
        message: &str,
        step_id: Option<&str>,
    ) -> bool {
        self.send(
            step_id.map(str::to_owned),
            JobEventKind::Diagnostic {
                level,
                code: code.to_owned(),
                message: message.to_owned(),
            },
        )
    }

    fn send(&mut self, step_id: Option<String>, event: JobEventKind) -> bool {
        self.sequence += 1;
        self.sender
            .send_blocking(JobEvent {
                protocol_version: PROTOCOL_VERSION,
                job_id: self.id.clone(),
                sequence: self.sequence,
                elapsed_ms: self.started.elapsed().as_millis() as u64,
                step_id,
                event,
            })
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bunch_core::{AssetBinding, ExecutionBindings, ExecutorKind, Recipe};
    use bunch_planner::Planner;
    use bunch_protocol::JobSnapshot;

    use crate::foundation_capabilities;

    use super::*;

    fn plan(failure: Option<u8>) -> ExecutionPlan {
        let mut recipe = Recipe::foundation_demo();
        let Operation::FoundationSimulate(parameters) = &mut recipe.steps[0].operation;
        parameters.duration_ms = 100;
        parameters.fail_at_percent = failure;
        Planner
            .plan(
                &recipe,
                ExecutionBindings {
                    inputs: BTreeMap::from([(
                        "source".to_owned(),
                        AssetBinding {
                            locator: "fixture://source".to_owned(),
                            display_name: "source.png".to_owned(),
                        },
                    )]),
                    output_directory: "fixture://output".to_owned(),
                },
                &foundation_capabilities(),
                ExecutorKind::Native,
            )
            .expect("foundation plan")
    }

    #[test]
    fn successful_job_has_monotonic_events_and_one_terminal_state() {
        let handle = NativeRunner::new().submit(plan(None)).expect("job");
        let events = handle.events();
        let mut snapshot = JobSnapshot::new(handle.id().clone());
        let mut terminal = Vec::new();
        while let Ok(event) = events.recv_blocking() {
            snapshot.apply(&event).expect("valid event");
            if let Some(state) = event.terminal_state() {
                terminal.push(state);
                break;
            }
        }
        assert_eq!(terminal, [JobState::Succeeded]);
        assert_eq!(snapshot.artifacts.len(), 1);
        assert_eq!(snapshot.progress_completed, 100);
    }

    #[test]
    fn cancellation_reaches_cancelled_without_artifact() {
        let handle = NativeRunner::new().submit(plan(None)).expect("job");
        let events = handle.events();
        let mut snapshot = JobSnapshot::new(handle.id().clone());
        while let Ok(event) = events.recv_blocking() {
            let running = matches!(
                event.event,
                JobEventKind::StateChanged {
                    to: JobState::Running,
                    ..
                }
            );
            snapshot.apply(&event).expect("valid event");
            if running {
                handle.cancel();
            }
            if event.terminal_state().is_some() {
                break;
            }
        }
        assert_eq!(snapshot.state, Some(JobState::Cancelled));
        assert!(snapshot.artifacts.is_empty());
    }

    #[test]
    fn requested_failure_is_structured_and_terminal() {
        let handle = NativeRunner::new().submit(plan(Some(40))).expect("job");
        let events = handle.events();
        let mut snapshot = JobSnapshot::new(handle.id().clone());
        while let Ok(event) = events.recv_blocking() {
            snapshot.apply(&event).expect("valid event");
            if event.terminal_state().is_some() {
                break;
            }
        }
        assert_eq!(snapshot.state, Some(JobState::Failed));
        assert_eq!(snapshot.diagnostics.len(), 1);
        assert!(snapshot.artifacts.is_empty());
    }
}

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use bunch_core::{
    ArtifactManifest, ImageFormat, MetadataPolicy, Operation, ResizeMode, StepSource,
};
use bunch_planner::ExecutionPlan;
use bunch_protocol::{DiagnosticLevel, JobEvent, JobEventKind, JobId, JobState, PROTOCOL_VERSION};

use crate::CancellationToken;
use crate::adapters::imagemagick::{
    ADAPTER_ID as IMAGEMAGICK_ADAPTER_ID, ImageMagick, ProcessOutcome,
};
use crate::capabilities::FOUNDATION_ADAPTER_ID;
use crate::staging::StagedOutput;

#[derive(Clone, Debug)]
pub struct NativeRunner {
    next_id: Arc<AtomicU64>,
    image_magick: Option<ImageMagick>,
}

impl Default for NativeRunner {
    fn default() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            image_magick: ImageMagick::discover().ok(),
        }
    }
}

impl NativeRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&self, plan: ExecutionPlan) -> Result<JobHandle, RunnerError> {
        let Some(adapter_id) = plan
            .steps
            .first()
            .map(|step| step.capability.adapter.id.as_str())
        else {
            return Err(RunnerError::UnsupportedPlan);
        };
        if plan
            .steps
            .iter()
            .any(|step| step.capability.adapter.id != adapter_id)
        {
            return Err(RunnerError::UnsupportedPlan);
        }
        let adapter = match adapter_id {
            FOUNDATION_ADAPTER_ID => RunnerAdapter::Foundation,
            IMAGEMAGICK_ADAPTER_ID => RunnerAdapter::ImageMagick(
                self.image_magick
                    .clone()
                    .ok_or(RunnerError::ImageEngineUnavailable)?,
            ),
            _ => return Err(RunnerError::UnsupportedPlan),
        };

        let numeric_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = JobId(format!("job-{numeric_id:06}"));
        let cancellation = CancellationToken::new();
        let thread_cancellation = cancellation.clone();
        let thread_id = id.clone();
        let (events, receiver) = async_channel::bounded(64);
        thread::Builder::new()
            .name(format!("bunch-{numeric_id:06}"))
            .spawn(move || match adapter {
                RunnerAdapter::Foundation => {
                    run_foundation_job(thread_id, plan, thread_cancellation, events);
                }
                RunnerAdapter::ImageMagick(engine) => {
                    run_image_job(thread_id, plan, thread_cancellation, events, &engine);
                }
            })
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
    ImageEngineUnavailable,
    Thread(String),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlan => {
                write!(formatter, "plan is not supported by the native runner")
            }
            Self::ImageEngineUnavailable => write!(formatter, "ImageMagick 7 is not available"),
            Self::Thread(error) => write!(formatter, "could not start job thread: {error}"),
        }
    }
}

enum RunnerAdapter {
    Foundation,
    ImageMagick(ImageMagick),
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
            Operation::ImageResize(_) => {
                emitter.diagnostic(
                    DiagnosticLevel::Error,
                    "foundation-operation-mismatch",
                    "The foundation adapter received an image operation.",
                    Some(&step.id),
                );
                emitter.change_state(JobState::Failed);
                return;
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
        source_properties: None,
        output_properties: None,
    }) {
        return;
    }
    emitter.change_state(JobState::Succeeded);
}

fn run_image_job(
    id: JobId,
    plan: ExecutionPlan,
    cancellation: CancellationToken,
    sender: Sender<JobEvent>,
    engine: &ImageMagick,
) {
    let mut emitter = Emitter::new(id.clone(), sender);
    if !emitter.change_state(JobState::Queued) || cancel_if_requested(&mut emitter, &cancellation) {
        return;
    }
    if !emitter.change_state(JobState::Preparing) {
        return;
    }
    let Some(step) = plan.steps.first() else {
        fail_job(
            &mut emitter,
            "image-empty-plan",
            "The image plan has no step",
            None,
        );
        return;
    };
    let Operation::ImageResize(parameters) = &step.operation else {
        fail_job(
            &mut emitter,
            "image-operation-mismatch",
            "The ImageMagick adapter received a non-image operation",
            Some(&step.id),
        );
        return;
    };
    let Some(input_path) = input_path(&plan, step.inputs.first()) else {
        fail_job(
            &mut emitter,
            "image-input-binding",
            "The image input binding could not be resolved",
            Some(&step.id),
        );
        return;
    };
    if !emitter.progress(5, 100, "Probing source image", &step.id) {
        return;
    }
    let source_probe = match engine.validate_resize_input(&input_path) {
        Ok(probe) => probe,
        Err(error) => {
            fail_job(&mut emitter, "image-probe-failed", &error, Some(&step.id));
            return;
        }
    };
    if let Err(error) = engine.validate_parameters_for_probe(parameters, &source_probe) {
        fail_job(
            &mut emitter,
            "image-parameters-invalid",
            &error,
            Some(&step.id),
        );
        return;
    }
    if cancel_if_requested(&mut emitter, &cancellation) {
        return;
    }

    let output_directory = PathBuf::from(&plan.bindings.output_directory);
    let staged = match StagedOutput::prepare(
        &output_directory,
        &plan.output_name,
        plan.output.collision,
        &id.0,
    ) {
        Ok(staged) => staged,
        Err(error) => {
            fail_job(&mut emitter, "image-staging-failed", &error, Some(&step.id));
            return;
        }
    };
    if !emitter.change_state(JobState::Running)
        || !emitter.progress(15, 100, "Resizing with ImageMagick", &step.id)
    {
        staged.cleanup();
        return;
    }
    match engine.resize(&input_path, &staged.staging_path, parameters, &cancellation) {
        Ok(ProcessOutcome::Completed) => {}
        Ok(ProcessOutcome::Cancelled) => {
            staged.cleanup();
            cancel_if_requested(&mut emitter, &cancellation);
            return;
        }
        Err(error) => {
            staged.cleanup();
            fail_job(
                &mut emitter,
                "image-execution-failed",
                &error,
                Some(&step.id),
            );
            return;
        }
    }
    if !emitter.progress(85, 100, "Verifying staged artifact", &step.id)
        || !emitter.change_state(JobState::Finalizing)
    {
        staged.cleanup();
        return;
    }
    let output_probe = match engine.probe(&staged.staging_path) {
        Ok(probe) => probe,
        Err(error) => {
            staged.cleanup();
            fail_job(
                &mut emitter,
                "image-output-probe-failed",
                &error,
                Some(&step.id),
            );
            return;
        }
    };
    if let Err(error) = verify_output(parameters, &output_probe) {
        staged.cleanup();
        fail_job(
            &mut emitter,
            "image-output-mismatch",
            &error,
            Some(&step.id),
        );
        return;
    }
    if cancel_if_requested(&mut emitter, &cancellation) {
        staged.cleanup();
        return;
    }
    if let Err(error) = staged.publish() {
        staged.cleanup();
        fail_job(
            &mut emitter,
            "image-publication-failed",
            &error,
            Some(&step.id),
        );
        return;
    }

    let mut warnings = Vec::new();
    if parameters.metadata == MetadataPolicy::Strip {
        warnings
            .push("Profiles, comments, and privacy-sensitive metadata were stripped".to_owned());
    }
    if !parameters.format.preserves_alpha() && source_probe.has_alpha {
        warnings.push(format!(
            "Source alpha was flattened against #{:06X}",
            parameters.background_rgb
        ));
    }
    let adapter = &step.capability.adapter;
    if !emitter.progress(100, 100, "Artifact published", &step.id)
        || !emitter.artifact(ArtifactManifest {
            name: staged
                .final_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&plan.output_name)
                .to_owned(),
            locator: staged.final_path.to_string_lossy().into_owned(),
            media_type: parameters.format.media_type().to_owned(),
            byte_length: output_probe.file_size,
            adapter_id: adapter.id.clone(),
            engine_name: adapter.engine_name.clone(),
            engine_version: adapter.engine_version.clone(),
            warnings,
            source_properties: Some(source_probe),
            output_properties: Some(output_probe),
        })
    {
        return;
    }
    emitter.change_state(JobState::Succeeded);
}

fn input_path(plan: &ExecutionPlan, source: Option<&StepSource>) -> Option<PathBuf> {
    let StepSource::Input(input_id) = source? else {
        return None;
    };
    plan.bindings
        .inputs
        .get(input_id)
        .map(|binding| PathBuf::from(&binding.locator))
}

fn verify_output(
    parameters: &bunch_core::ImageResizeParameters,
    output: &bunch_core::ImageProbe,
) -> Result<(), String> {
    let expected_format = match parameters.format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Webp => "WEBP",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Jp2 => "JP2",
        ImageFormat::Jxl => "JXL",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Tga => "TGA",
        ImageFormat::Tiff => "TIFF",
    };
    if output.format != expected_format {
        return Err(format!(
            "expected {expected_format} output, engine produced {}",
            output.format
        ));
    }
    if parameters.mode != ResizeMode::Fit || parameters.pad_to_size {
        if output.width != parameters.width || output.height != parameters.height {
            return Err(format!(
                "expected exact {}x{} fill output, got {}x{}",
                parameters.width, parameters.height, output.width, output.height
            ));
        }
    } else if output.width > parameters.width || output.height > parameters.height {
        return Err(format!(
            "fit output {}x{} exceeds {}x{} bounds",
            output.width, output.height, parameters.width, parameters.height
        ));
    }
    if parameters.metadata == MetadataPolicy::Strip && !output.profile_names.is_empty() {
        return Err(format!(
            "metadata strip requested but output retained profiles: {}",
            output.profile_names.join(", ")
        ));
    }
    Ok(())
}

fn fail_job(emitter: &mut Emitter, code: &str, message: &str, step_id: Option<&str>) {
    emitter.diagnostic(DiagnosticLevel::Error, code, message, step_id);
    emitter.change_state(JobState::Failed);
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
        self.send(
            None,
            JobEventKind::ArtifactProduced {
                artifact: Box::new(artifact),
            },
        )
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
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use bunch_core::{
        AssetBinding, CollisionPolicy, ExecutionBindings, ExecutorKind, ImageFormat,
        MetadataPolicy, Operation, Recipe,
    };
    use bunch_planner::Planner;
    use bunch_protocol::JobSnapshot;
    use image::{ColorType, ImageFormat as FixtureFormat};

    use crate::{foundation_capabilities, native_capabilities};

    use super::*;

    fn plan(failure: Option<u8>) -> ExecutionPlan {
        let mut recipe = Recipe::foundation_demo();
        let Operation::FoundationSimulate(parameters) = &mut recipe.steps[0].operation else {
            panic!("foundation recipe operation");
        };
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

    fn test_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bunch-image-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        let mut pixels = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    (x % 251) as u8,
                    (y % 241) as u8,
                    ((x + y) % 239) as u8,
                    if (x + y) % 3 == 0 { 128 } else { 255 },
                ]);
            }
        }
        image::save_buffer_with_format(
            path,
            &pixels,
            width,
            height,
            ColorType::Rgba8,
            FixtureFormat::Png,
        )
        .expect("PNG fixture");
    }

    fn image_plan(
        input: &Path,
        collision: CollisionPolicy,
        metadata: MetadataPolicy,
    ) -> ExecutionPlan {
        let mut recipe = Recipe::image_resize();
        recipe.output.name_template = "{stem}.optimized.png".to_owned();
        recipe.output.collision = collision;
        let Operation::ImageResize(parameters) = &mut recipe.steps[0].operation else {
            panic!("image recipe operation");
        };
        parameters.width = 64;
        parameters.height = 64;
        parameters.format = ImageFormat::Png;
        parameters.metadata = metadata;
        let display_name = input
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture filename")
            .to_owned();
        Planner
            .plan(
                &recipe,
                ExecutionBindings {
                    inputs: BTreeMap::from([(
                        "source".to_owned(),
                        AssetBinding {
                            locator: input.to_string_lossy().into_owned(),
                            display_name,
                        },
                    )]),
                    output_directory: input
                        .parent()
                        .expect("fixture parent")
                        .to_string_lossy()
                        .into_owned(),
                },
                &native_capabilities(),
                ExecutorKind::Native,
            )
            .expect("image plan")
    }

    fn collect(handle: &JobHandle) -> JobSnapshot {
        let events = handle.events();
        let mut snapshot = JobSnapshot::new(handle.id().clone());
        while let Ok(event) = events.recv_blocking() {
            snapshot.apply(&event).expect("valid event");
            if event.terminal_state().is_some() {
                break;
            }
        }
        snapshot
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

    #[test]
    fn real_image_job_preserves_unicode_input_and_increments_collision() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        let directory = test_directory("unicode").join("unicode-\u{56fe}");
        fs::create_dir_all(&directory).expect("Unicode directory");
        let base = directory.join("base.png");
        let input = directory.join("source-\u{56fe}.png");
        write_png(&base, 96, 48);

        let profile = engine
            .executable()
            .parent()
            .map(|parent| parent.join("sRGB.icc"));
        let mut fixture = Command::new(engine.executable());
        fixture
            .arg(&base)
            .args(["-set", "comment", "bunch-metadata-fixture"])
            .args(["-set", "orientation", "RightTop"]);
        if let Some(profile) = profile.as_ref().filter(|path| path.is_file()) {
            fixture.arg("-profile").arg(profile);
        }
        let status = fixture.arg(&input).status().expect("metadata fixture");
        assert!(status.success());
        let source_probe = engine.probe(&input).expect("source probe");
        assert_eq!(source_probe.orientation, "RightTop");
        if profile.as_ref().is_some_and(|path| path.is_file()) {
            assert!(source_probe.profile_names.iter().any(|name| name == "icc"));
        }

        let original = fs::read(&input).expect("source bytes");
        let requested = directory.join("source-\u{56fe}.optimized.png");
        fs::write(&requested, b"do not replace").expect("collision fixture");
        let plan = image_plan(&input, CollisionPolicy::Increment, MetadataPolicy::Strip);
        let handle = NativeRunner::new().submit(plan).expect("image job");
        let snapshot = collect(&handle);

        assert_eq!(snapshot.state, Some(JobState::Succeeded));
        assert_eq!(fs::read(&input).expect("unchanged source"), original);
        assert_eq!(
            fs::read(&requested).expect("unchanged collision"),
            b"do not replace"
        );
        let artifact = snapshot.artifacts.first().expect("artifact");
        assert_eq!(artifact.name, "source-\u{56fe}.optimized (2).png");
        let output = PathBuf::from(&artifact.locator);
        assert!(output.is_file());
        let properties = artifact.output_properties.as_ref().expect("output probe");
        assert_eq!((properties.width, properties.height), (64, 32));
        assert_eq!(properties.format, "PNG");
        assert!(properties.profile_names.is_empty());

        let comment = Command::new(engine.executable())
            .args(["identify", "-quiet", "-format", "%[comment]"])
            .arg(&output)
            .output()
            .expect("comment probe");
        assert!(comment.status.success());
        assert!(comment.stdout.is_empty());

        let preserve_plan =
            image_plan(&input, CollisionPolicy::Increment, MetadataPolicy::Preserve);
        let preserve_handle = NativeRunner::new()
            .submit(preserve_plan)
            .expect("preserve image job");
        let preserve_snapshot = collect(&preserve_handle);
        assert_eq!(preserve_snapshot.state, Some(JobState::Succeeded));
        let preserve_artifact = preserve_snapshot
            .artifacts
            .first()
            .expect("preserved artifact");
        assert_eq!(preserve_artifact.name, "source-\u{56fe}.optimized (3).png");
        if profile.as_ref().is_some_and(|path| path.is_file()) {
            assert!(
                preserve_artifact
                    .output_properties
                    .as_ref()
                    .expect("preserved output probe")
                    .profile_names
                    .iter()
                    .any(|name| name == "icc")
            );
        }
        let preserved_comment = Command::new(engine.executable())
            .args(["identify", "-quiet", "-format", "%[comment]"])
            .arg(&preserve_artifact.locator)
            .output()
            .expect("preserved comment probe");
        assert!(preserved_comment.status.success());
        assert_eq!(preserved_comment.stdout, b"bunch-metadata-fixture");
        assert!(
            !fs::read_dir(&directory)
                .expect("directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".partial."))
        );
        fs::remove_dir_all(directory.parent().expect("test root")).expect("cleanup");
    }

    #[test]
    fn image_cancellation_removes_staging_and_publishes_nothing() {
        if ImageMagick::discover().is_err() {
            return;
        }
        let directory = test_directory("cancel");
        let input = directory.join("large-alpha.png");
        write_png(&input, 2_048, 2_048);
        let plan = image_plan(&input, CollisionPolicy::Increment, MetadataPolicy::Strip);
        let expected = directory.join("large-alpha.optimized.png");
        let handle = NativeRunner::new().submit(plan).expect("image job");
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
        assert!(!expected.exists());
        assert!(snapshot.artifacts.is_empty());
        assert!(
            !fs::read_dir(&directory)
                .expect("directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".partial."))
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }
}

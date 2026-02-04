use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bunch_core::{AssetBinding, CapabilitySet, ExecutionBindings, ExecutorKind, Operation, Recipe};
use bunch_native::{
    CancellationToken, EngineDiagnostic, NativeRunner, engine_diagnostics, foundation_capabilities,
};
use bunch_planner::Planner;
use bunch_protocol::{JobEvent, JobId, JobSnapshot};
use gpui::*;

use crate::assets::Assets;
use crate::settings::{DesktopSettings, SettingsWorker};
use crate::theme::{MONO_FONT, UI_FONT};

#[derive(Clone, Debug)]
pub(crate) struct AssetEntry {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Debug)]
pub(crate) struct ActiveJob {
    pub snapshot: JobSnapshot,
    pub cancellation: CancellationToken,
}

pub(crate) struct AppModel {
    pub assets: Vec<AssetEntry>,
    pub selected_asset: Option<usize>,
    pub recipe: Recipe,
    pub capabilities: CapabilitySet,
    pub diagnostics: Vec<EngineDiagnostic>,
    pub active_job: Option<ActiveJob>,
    pub status: String,
    pub settings: DesktopSettings,
    runner: NativeRunner,
    settings_worker: Option<SettingsWorker>,
}

impl AppModel {
    fn new(_: &mut Context<Self>) -> Self {
        let (settings, settings_status) = match crate::settings::load() {
            Ok(settings) => (settings, "READY".to_owned()),
            Err(error) => (DesktopSettings::default(), format!("SETTINGS: {error}")),
        };
        let settings_worker = SettingsWorker::start().ok();
        let mut recipe = Recipe::foundation_demo();
        let Operation::FoundationSimulate(parameters) = &mut recipe.steps[0].operation;
        parameters.duration_ms = settings.simulation_duration_ms;
        Self {
            assets: Vec::new(),
            selected_asset: None,
            recipe,
            capabilities: foundation_capabilities(),
            diagnostics: engine_diagnostics(),
            active_job: None,
            status: settings_status,
            settings,
            runner: NativeRunner::new(),
            settings_worker,
        }
    }

    pub(crate) fn selected(&self) -> Option<&AssetEntry> {
        self.selected_asset.and_then(|index| self.assets.get(index))
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        let Operation::FoundationSimulate(parameters) = &self.recipe.steps[0].operation;
        parameters.duration_ms
    }

    pub(crate) fn can_run(&self) -> bool {
        self.selected().is_some()
            && self.active_job.as_ref().is_none_or(|job| {
                job.snapshot
                    .state
                    .is_none_or(bunch_protocol::JobState::is_terminal)
            })
    }

    pub(crate) fn can_cancel(&self) -> bool {
        self.active_job
            .as_ref()
            .is_some_and(|job| job.snapshot.state.is_some_and(|state| !state.is_terminal()))
    }

    pub(crate) fn open_files(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        #[cfg(windows)]
        {
            if let Some(paths) = rfd::FileDialog::new()
                .set_title("Open project assets")
                .pick_files()
            {
                for path in paths {
                    if self.assets.iter().any(|asset| asset.path == path) {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("unnamed asset")
                        .to_owned();
                    self.assets.push(AssetEntry { path, name });
                }
                if self.selected_asset.is_none() && !self.assets.is_empty() {
                    self.selected_asset = Some(0);
                }
                self.status = format!("{} ASSET(S) READY", self.assets.len());
            }
        }
        #[cfg(not(windows))]
        {
            self.status = "FILE PICKER UNAVAILABLE ON THIS BUILD".to_owned();
        }
        cx.notify();
    }

    pub(crate) fn clear_assets(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.can_cancel() {
            self.assets.clear();
            self.selected_asset = None;
            self.status = "READY".to_owned();
            cx.notify();
        }
    }

    pub(crate) fn select_asset(
        &mut self,
        index: usize,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index < self.assets.len() {
            self.selected_asset = Some(index);
            self.status = self.assets[index].name.to_ascii_uppercase();
            cx.notify();
        }
    }

    pub(crate) fn duration_fast(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_duration(500, cx);
    }

    pub(crate) fn duration_normal(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_duration(1_200, cx);
    }

    pub(crate) fn duration_slow(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_duration(3_000, cx);
    }

    fn set_duration(&mut self, duration_ms: u64, cx: &mut Context<Self>) {
        let Operation::FoundationSimulate(parameters) = &mut self.recipe.steps[0].operation;
        parameters.duration_ms = duration_ms;
        self.settings.simulation_duration_ms = duration_ms;
        if let Some(worker) = &self.settings_worker {
            worker.queue(self.settings.clone());
        }
        self.status = format!("DURATION {duration_ms} MS");
        cx.notify();
    }

    pub(crate) fn start_job(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.can_run() {
            return;
        }
        let Some(asset) = self.selected().cloned() else {
            return;
        };
        match self
            .plan_for(&asset)
            .and_then(|plan| self.runner.submit(plan).map_err(|error| error.to_string()))
        {
            Ok(handle) => {
                let receiver = handle.events();
                let cancellation = handle.cancellation_token();
                let job_id = handle.id().clone();
                self.active_job = Some(ActiveJob {
                    snapshot: JobSnapshot::new(job_id),
                    cancellation,
                });
                self.status = "JOB QUEUED".to_owned();
                cx.spawn(async move |this, cx| {
                    while let Ok(event) = receiver.recv().await {
                        let terminal = event.terminal_state().is_some();
                        let result = this.update(cx, |this, cx| {
                            this.apply_job_event(&event);
                            cx.notify();
                        });
                        if terminal || result.is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => self.status = format!("PLAN ERROR: {error}"),
        }
        cx.notify();
    }

    pub(crate) fn cancel_job(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = &self.active_job
            && job.snapshot.state.is_some_and(|state| !state.is_terminal())
        {
            job.cancellation.cancel();
            self.status = "CANCELLATION REQUESTED".to_owned();
            cx.notify();
        }
    }

    fn plan_for(&self, asset: &AssetEntry) -> Result<bunch_planner::ExecutionPlan, String> {
        let input = self
            .recipe
            .inputs
            .first()
            .ok_or_else(|| "recipe has no input slot".to_owned())?;
        let output_directory = asset
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .into_owned();
        Planner
            .plan(
                &self.recipe,
                ExecutionBindings {
                    inputs: BTreeMap::from([(
                        input.id.clone(),
                        AssetBinding {
                            locator: asset.path.to_string_lossy().into_owned(),
                            display_name: asset.name.clone(),
                        },
                    )]),
                    output_directory,
                },
                &self.capabilities,
                ExecutorKind::Native,
            )
            .map_err(|error| error.to_string())
    }

    fn apply_job_event(&mut self, event: &JobEvent) {
        let Some(job) = &mut self.active_job else {
            return;
        };
        if event.job_id != job.snapshot.job_id {
            return;
        }
        match job.snapshot.apply(event) {
            Ok(()) => {
                self.status = event_status(event, &job.snapshot.job_id);
            }
            Err(error) => {
                self.status = format!("PROTOCOL ERROR: {error}");
            }
        }
    }
}

fn event_status(event: &JobEvent, job_id: &JobId) -> String {
    event.terminal_state().map_or_else(
        || {
            format!(
                "{} / EVENT {}",
                job_id.0.to_ascii_uppercase(),
                event.sequence
            )
        },
        |state| format!("{} / {state:?}", job_id.0.to_ascii_uppercase()),
    )
}

pub(crate) fn run() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        let component_theme = gpui_component::Theme::global_mut(cx);
        component_theme.radius = px(2.0);
        component_theme.radius_lg = px(2.0);
        component_theme.font_family = UI_FONT.into();
        component_theme.mono_font_family = MONO_FONT.into();

        let bounds = Bounds::centered(None, size(px(1160.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.0), px(620.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Bunch of Tools".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                app_id: Some("bunch-of-tools".to_owned()),
                ..WindowOptions::default()
            },
            |window, cx| {
                let model = cx.new(AppModel::new);
                cx.new(|cx| gpui_component::Root::new(model, window, cx))
            },
        )
        .expect("Bunch of Tools window must open");
        cx.activate(true);
    });
}

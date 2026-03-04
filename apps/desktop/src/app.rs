use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bunch_core::{
    AssetBinding, CapabilitySet, CropParameters, EncodingMode, ExecutionBindings, ExecutorKind,
    ImageFormat, ImageResizeParameters, MetadataPolicy, Operation, OperationKind, Recipe,
    ResizeMode,
};
use bunch_native::{
    CancellationToken, EngineDiagnostic, ImagePreview, NativeRunner, engine_diagnostics,
    native_capabilities, request_image_preview,
};
use bunch_planner::Planner;
use bunch_protocol::{JobEvent, JobId, JobSnapshot};
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::slider::{SliderEvent, SliderState, SliderValue};
use image::Frame;
use smallvec::smallvec;

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
    pub asset_index: usize,
    pub snapshot: JobSnapshot,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InspectorSection {
    Resize,
    Crop,
    Encode,
}

#[derive(Clone, Copy)]
enum NumericField {
    Width,
    Height,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CropHandle {
    New,
    Move,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CropDrag {
    handle: CropHandle,
    origin: (f32, f32),
    initial: CropParameters,
}

pub(crate) struct AppModel {
    pub assets: Vec<AssetEntry>,
    pub focused_asset: Option<usize>,
    pub selected_assets: BTreeSet<usize>,
    pub preview: Option<ImagePreview>,
    pub preview_source_texture: Option<Arc<RenderImage>>,
    pub preview_result_texture: Option<Arc<RenderImage>>,
    pub preview_loading: bool,
    pub preview_error: Option<String>,
    pub recipe: Recipe,
    pub capabilities: CapabilitySet,
    pub diagnostics: Vec<EngineDiagnostic>,
    pub jobs: Vec<ActiveJob>,
    pub show_result: bool,
    pub output_directory: Option<PathBuf>,
    pub status: String,
    pub dialog_open: bool,
    pub settings: DesktopSettings,
    pub width_input: Entity<InputState>,
    pub height_input: Entity<InputState>,
    pub quality_slider: Entity<SliderState>,
    pub dimensions_linked: bool,
    pub active_tool: InspectorSection,
    pub crop_enabled: bool,
    pub assets_open: bool,
    pub inspector_open: bool,
    pub queue_open: bool,
    pub canvas_zoom: f32,
    pub canvas_pan_x: f32,
    pub canvas_pan_y: f32,
    pub canvas_drag: Option<(Point<Pixels>, f32, f32)>,
    pub crop_drag: Option<CropDrag>,
    working_files: Vec<PathBuf>,
    runner: NativeRunner,
    preview_generation: u64,
    preview_cancellation: Option<CancellationToken>,
    settings_worker: Option<SettingsWorker>,
    syncing_inputs: bool,
    dimensions_initialized: bool,
    dimensions_touched: bool,
    linked_ratio: (u32, u32),
    pending_dimension_inputs: Option<(u32, u32)>,
    _subscriptions: Vec<Subscription>,
}

impl AppModel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, settings_status) = match crate::settings::load() {
            Ok(settings) => (settings, "Ready".to_owned()),
            Err(error) => (DesktopSettings::default(), format!("Settings: {error}")),
        };
        let settings_worker = SettingsWorker::start().ok();
        let mut recipe = Recipe::image_resize();
        apply_settings(&mut recipe, &settings);
        let parameters = image_parameters(&recipe).clone();

        let width_input = numeric_input(parameters.width, window, cx);
        let height_input = numeric_input(parameters.height, window, cx);

        let quality_slider = slider(1.0, 100.0, 1.0, f32::from(parameters.quality), cx);

        let mut subscriptions = Vec::new();
        for (input, field) in [
            (&width_input, NumericField::Width),
            (&height_input, NumericField::Height),
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.handle_numeric_input(field, input, window, cx);
                    }
                },
            ));
        }
        subscriptions.push(
            cx.subscribe(
                &quality_slider,
                |this, _, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(value) => this.handle_quality_slider(*value, cx),
                },
            ),
        );

        let mut model = Self {
            assets: Vec::new(),
            focused_asset: None,
            selected_assets: BTreeSet::new(),
            preview: None,
            preview_source_texture: None,
            preview_result_texture: None,
            preview_loading: false,
            preview_error: None,
            recipe,
            capabilities: native_capabilities(),
            diagnostics: engine_diagnostics(),
            jobs: Vec::new(),
            show_result: true,
            output_directory: None,
            status: settings_status,
            dialog_open: false,
            settings,
            width_input,
            height_input,
            quality_slider,
            dimensions_linked: true,
            active_tool: InspectorSection::Resize,
            crop_enabled: false,
            assets_open: true,
            inspector_open: true,
            queue_open: true,
            canvas_zoom: 1.0,
            canvas_pan_x: 0.0,
            canvas_pan_y: 0.0,
            canvas_drag: None,
            crop_drag: None,
            working_files: Vec::new(),
            runner: NativeRunner::new(),
            preview_generation: 0,
            preview_cancellation: None,
            settings_worker,
            syncing_inputs: false,
            dimensions_initialized: false,
            dimensions_touched: false,
            linked_ratio: (parameters.width, parameters.height),
            pending_dimension_inputs: None,
            _subscriptions: subscriptions,
        };
        if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from)
            && path.is_file()
        {
            model.add_paths([path]);
            model.begin_preview(cx);
        }
        model
    }

    pub(crate) fn selected(&self) -> Option<&AssetEntry> {
        self.focused_asset.and_then(|index| self.assets.get(index))
    }

    pub(crate) fn image_parameters(&self) -> &ImageResizeParameters {
        image_parameters(&self.recipe)
    }

    fn image_parameters_mut(&mut self) -> &mut ImageResizeParameters {
        match &mut self.recipe.steps[0].operation {
            Operation::ImageResize(parameters) => parameters,
            Operation::FoundationSimulate(_) => unreachable!("desktop uses image recipe"),
        }
    }

    pub(crate) fn can_export(&self) -> bool {
        !self.selected_assets.is_empty()
            && self
                .capabilities
                .select(OperationKind::ImageResize, ExecutorKind::Native)
                .is_some()
            && !self.can_cancel()
    }

    pub(crate) fn can_cancel(&self) -> bool {
        self.jobs
            .iter()
            .any(|job| job.snapshot.state.is_some_and(|state| !state.is_terminal()))
    }

    pub(crate) fn open_files(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.can_cancel() {
            return;
        }
        #[cfg(windows)]
        {
            self.dialog_open = true;
            self.status = "Opening files".to_owned();
            let picker = cx
                .background_executor()
                .spawn(async { run_picker_helper("--bunch-pick-files") });
            cx.spawn(async move |this, cx| {
                let paths = picker.await;
                let _ = this.update(cx, |this, cx| {
                    this.dialog_open = false;
                    match paths {
                        Ok(paths) if !paths.is_empty() => {
                            this.import_paths(paths, cx);
                        }
                        Ok(_) => this.status = "Ready".to_owned(),
                        Err(error) => this.status = format!("Import: {error}"),
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        #[cfg(not(windows))]
        {
            self.status = "File picker unavailable on this build".to_owned();
        }
        cx.notify();
    }

    pub(crate) fn drop_files(
        &mut self,
        paths: &ExternalPaths,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.can_cancel() {
            return;
        }
        self.import_paths(
            paths.paths().iter().filter(|path| path.is_file()).cloned(),
            cx,
        );
    }

    fn import_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>, cx: &mut Context<Self>) {
        let before = self.assets.len();
        self.add_paths(paths);
        let added = self.assets.len() - before;
        if added == 0 {
            self.status = "No new images".to_owned();
            return;
        }
        if self.focused_asset.is_none() {
            self.focused_asset = Some(before);
        }
        self.status = format!("{added} image{} added", if added == 1 { "" } else { "s" });
        self.begin_preview(cx);
    }

    fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if self.assets.iter().any(|asset| asset.path == path) {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("unnamed image")
                .to_owned();
            let index = self.assets.len();
            self.assets.push(AssetEntry { path, name });
            self.selected_assets.insert(index);
            self.focused_asset.get_or_insert(index);
        }
    }

    pub(crate) fn clear_assets(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_cancel() {
            return;
        }
        self.cancel_preview();
        self.remove_current_preview();
        self.assets.clear();
        self.selected_assets.clear();
        self.focused_asset = None;
        self.jobs.clear();
        self.preview_error = None;
        self.preview_loading = false;
        self.output_directory = None;
        self.status = "Ready".to_owned();
        self.dimensions_initialized = false;
        self.dimensions_touched = false;
        self.linked_ratio = (
            self.image_parameters().width,
            self.image_parameters().height,
        );
        self.pending_dimension_inputs = None;
        self.fit_canvas(cx);
        cx.notify();
    }

    pub(crate) fn focus_asset(
        &mut self,
        index: usize,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index < self.assets.len() && self.focused_asset != Some(index) {
            self.focused_asset = Some(index);
            self.remove_current_preview();
            self.fit_canvas(cx);
            self.begin_preview(cx);
        }
    }

    pub(crate) fn toggle_asset_selection(
        &mut self,
        index: usize,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selected_assets.remove(&index) {
            self.selected_assets.insert(index);
        }
        self.status = format!("{} selected for export", self.selected_assets.len());
        cx.notify();
    }

    pub(crate) fn select_all(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_assets = (0..self.assets.len()).collect();
        self.status = format!("{} selected for export", self.selected_assets.len());
        cx.notify();
    }

    pub(crate) fn select_none(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_assets.clear();
        self.status = "No images selected for export".to_owned();
        cx.notify();
    }

    fn begin_preview(&mut self, cx: &mut Context<Self>) {
        self.cancel_preview();
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let generation = self.preview_generation;
        self.start_preview(generation, cx);
    }

    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.cancel_preview();
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let generation = self.preview_generation;
        self.preview_loading = self.selected().is_some();
        self.preview_error = None;
        let timer = cx.background_executor().timer(Duration::from_millis(70));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if generation == this.preview_generation {
                    this.start_preview(generation, cx);
                }
            });
        })
        .detach();
    }

    fn start_preview(&mut self, generation: u64, cx: &mut Context<Self>) {
        let Some(asset) = self.selected().cloned() else {
            self.preview_loading = false;
            return;
        };
        self.preview_loading = true;
        self.preview_error = None;
        self.status = format!("Rendering {}", asset.name);
        let reusable_source = self
            .preview
            .as_ref()
            .map(|preview| (preview.source_path.clone(), preview.source.clone()));
        match request_image_preview(
            asset.path,
            self.image_parameters().clone(),
            generation,
            reusable_source,
        ) {
            Ok(handle) => {
                let receiver = handle.receiver();
                self.preview_cancellation = Some(handle.cancellation_token());
                cx.spawn(async move |this, cx| {
                    if let Ok(result) = receiver.recv().await {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_preview_result(generation, result, cx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Err(error) => {
                self.preview_loading = false;
                self.preview_error = Some(error.clone());
                self.status = format!("Preview: {error}");
            }
        }
        cx.notify();
    }

    fn apply_preview_result(
        &mut self,
        generation: u64,
        result: Result<ImagePreview, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.preview_generation {
            if let Ok(preview) = result {
                remove_preview_files(
                    &preview,
                    self.preview.as_ref().map(|item| item.source_path.as_path()),
                );
            }
            return;
        }
        self.preview_cancellation = None;
        match result {
            Ok(preview) => {
                self.status = "Preparing preview".to_owned();
                let source_path = preview.source_path.clone();
                let result_path = preview.result_path.clone();
                let reusable_source_texture = self.preview.as_ref().and_then(|current| {
                    (current.source_path == source_path)
                        .then(|| self.preview_source_texture.clone())
                        .flatten()
                });
                let decode = cx.background_executor().spawn(async move {
                    Ok::<_, String>((
                        reusable_source_texture
                            .map_or_else(|| decode_preview_texture(&source_path), Ok)?,
                        decode_preview_texture(&result_path)?,
                    ))
                });
                cx.spawn(async move |this, cx| {
                    let decoded = decode.await;
                    let _ = this.update(cx, |this, cx| {
                        this.commit_preview(generation, preview, decoded, cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            Err(error) => {
                self.preview_loading = false;
                self.preview_error = Some(error.clone());
                self.status = format!("Preview: {error}");
            }
        }
    }

    fn commit_preview(
        &mut self,
        generation: u64,
        preview: ImagePreview,
        decoded: Result<(Arc<RenderImage>, Arc<RenderImage>), String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.preview_generation {
            remove_preview_files(
                &preview,
                self.preview.as_ref().map(|item| item.source_path.as_path()),
            );
            return;
        }
        let (source_texture, result_texture) = match decoded {
            Ok(textures) => textures,
            Err(error) => {
                self.preview_loading = false;
                self.preview_error = Some(error.clone());
                self.status = format!("Preview: {error}");
                remove_preview_files(
                    &preview,
                    self.preview.as_ref().map(|item| item.source_path.as_path()),
                );
                return;
            }
        };
        let previous = self.preview.take();
        if let Some(previous) = previous {
            remove_preview_files(&previous, Some(&preview.source_path));
        }
        let initialize_dimensions = !self.dimensions_initialized && !self.dimensions_touched;
        if initialize_dimensions {
            let source_dimensions = (preview.source.width, preview.source.height);
            let parameters = self.image_parameters_mut();
            parameters.width = source_dimensions.0;
            parameters.height = source_dimensions.1;
            self.linked_ratio = source_dimensions;
            self.pending_dimension_inputs = Some(source_dimensions);
            self.dimensions_initialized = true;
        } else if !self.dimensions_initialized {
            self.dimensions_initialized = true;
        }
        self.status = format!(
            "{} {}x{} -> {} {}x{} / {}",
            preview.source.format,
            preview.source.width,
            preview.source.height,
            preview.result.format,
            preview.result.width,
            preview.result.height,
            format_bytes(preview.result.file_size)
        );
        self.preview = Some(preview);
        self.preview_source_texture = Some(source_texture);
        self.preview_result_texture = Some(result_texture);
        self.preview_loading = false;
        self.preview_error = None;
        if initialize_dimensions {
            self.persist_parameters();
            self.schedule_preview(cx);
        }
    }

    fn cancel_preview(&mut self) {
        if let Some(cancellation) = self.preview_cancellation.take() {
            cancellation.cancel();
        }
    }

    fn remove_current_preview(&mut self) {
        if let Some(preview) = self.preview.take() {
            remove_preview_files(&preview, None);
        }
        self.preview_source_texture = None;
        self.preview_result_texture = None;
    }

    fn handle_numeric_input(
        &mut self,
        field: NumericField,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.syncing_inputs {
            return;
        }
        let value = input.read(cx).value().to_string();
        match field {
            NumericField::Width | NumericField::Height => {
                let Ok(value) = value.parse::<u32>() else {
                    return;
                };
                if !(1..=8_192).contains(&value) {
                    return;
                }
                let is_width = matches!(field, NumericField::Width);
                let dimensions_linked = self.dimensions_linked;
                let linked_ratio = self.linked_ratio;
                let DimensionUpdate::Changed { linked_value } = apply_dimension_input(
                    self.image_parameters_mut(),
                    is_width,
                    value,
                    dimensions_linked,
                    linked_ratio.0,
                    linked_ratio.1,
                ) else {
                    return;
                };
                self.dimensions_touched = true;

                if let Some(linked) = linked_value {
                    self.syncing_inputs = true;
                    let linked_input = if is_width {
                        &self.height_input
                    } else {
                        &self.width_input
                    };
                    linked_input.update(cx, |input, cx| {
                        input.set_value(linked.to_string(), window, cx)
                    });
                    self.syncing_inputs = false;
                }
                self.parameters_changed(cx);
            }
        }
    }

    fn handle_quality_slider(&mut self, value: SliderValue, cx: &mut Context<Self>) {
        let value = value.end().round() as i16;
        self.image_parameters_mut().quality = value.clamp(1, 100) as u8;
        self.parameters_changed(cx);
    }

    fn parameters_changed(&mut self, cx: &mut Context<Self>) {
        if self.image_parameters().encoding == EncodingMode::Lossless
            && !self.image_parameters().format.supports_lossless_choice()
        {
            self.image_parameters_mut().encoding = EncodingMode::Automatic;
        }
        self.persist_parameters();
        self.status = "Updating result preview".to_owned();
        self.schedule_preview(cx);
        cx.notify();
    }

    pub(crate) fn set_format(&mut self, format: ImageFormat, cx: &mut Context<Self>) {
        let parameters = self.image_parameters_mut();
        parameters.format = format;
        parameters.encoding = EncodingMode::Automatic;
        self.recipe.output.name_template = format!("{{stem}}.optimized.{}", format.extension());
        self.parameters_changed(cx);
    }

    pub(crate) fn set_metadata(&mut self, policy: MetadataPolicy, cx: &mut Context<Self>) {
        self.image_parameters_mut().metadata = policy;
        self.parameters_changed(cx);
    }

    pub(crate) fn toggle_dimensions_link(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dimensions_linked = !self.dimensions_linked;
        if self.dimensions_linked {
            self.linked_ratio = self.source_dimensions().unwrap_or((
                self.image_parameters().width,
                self.image_parameters().height,
            ));
            self.image_parameters_mut().mode = ResizeMode::Fit;
        } else {
            self.image_parameters_mut().mode = ResizeMode::Stretch;
        }
        self.parameters_changed(cx);
    }

    pub(crate) fn set_active_tool(&mut self, tool: InspectorSection, cx: &mut Context<Self>) {
        self.active_tool = tool;
        if tool == InspectorSection::Crop {
            self.show_result = false;
            self.fit_canvas(cx);
            if self.image_parameters().crop.is_none()
                && let Some((width, height)) = self.source_dimensions()
            {
                self.image_parameters_mut().crop = Some(CropParameters {
                    x: 0,
                    y: 0,
                    width,
                    height,
                });
            }
            self.crop_enabled = true;
        }
        cx.notify();
    }

    pub(crate) fn sync_pending_dimension_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((width, height)) = self.pending_dimension_inputs.take() else {
            return;
        };
        self.syncing_inputs = true;
        for (input, value) in [(&self.width_input, width), (&self.height_input, height)] {
            input.update(cx, |input, cx| {
                input.set_value(value.to_string(), window, cx)
            });
        }
        self.syncing_inputs = false;
    }

    pub(crate) fn toggle_crop(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.crop_enabled = !self.crop_enabled;
        if self.crop_enabled {
            if let Some((width, height)) = self.source_dimensions() {
                self.image_parameters_mut().crop = Some(CropParameters {
                    x: 0,
                    y: 0,
                    width,
                    height,
                });
            }
            self.show_result = false;
            self.fit_canvas(cx);
        } else {
            self.image_parameters_mut().crop = None;
        }
        self.parameters_changed(cx);
    }

    pub(crate) fn reset_crop(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((width, height)) = self.source_dimensions() else {
            return;
        };
        self.crop_enabled = true;
        self.image_parameters_mut().crop = Some(CropParameters {
            x: 0,
            y: 0,
            width,
            height,
        });
        self.parameters_changed(cx);
    }

    pub(crate) fn rotate_left(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let rotation = self.image_parameters().rotation_degrees;
        self.image_parameters_mut().rotation_degrees = normalize_rotation(rotation - 90);
        self.parameters_changed(cx);
    }

    pub(crate) fn rotate_right(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let rotation = self.image_parameters().rotation_degrees;
        self.image_parameters_mut().rotation_degrees = normalize_rotation(rotation + 90);
        self.parameters_changed(cx);
    }

    pub(crate) fn toggle_flip_horizontal(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = !self.image_parameters().flip_horizontal;
        self.image_parameters_mut().flip_horizontal = value;
        self.parameters_changed(cx);
    }

    pub(crate) fn toggle_flip_vertical(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = !self.image_parameters().flip_vertical;
        self.image_parameters_mut().flip_vertical = value;
        self.parameters_changed(cx);
    }

    fn persist_parameters(&mut self) {
        let parameters = self.image_parameters().clone();
        self.settings.image_width = parameters.width;
        self.settings.image_height = parameters.height;
        self.settings.image_format = parameters.format;
        self.settings.image_quality = parameters.quality;
        self.settings.resize_mode = parameters.mode;
        self.settings.allow_upscale = parameters.allow_upscale;
        self.settings.pad_to_size = parameters.pad_to_size;
        self.settings.rotation_degrees = parameters.rotation_degrees;
        self.settings.flip_horizontal = parameters.flip_horizontal;
        self.settings.flip_vertical = parameters.flip_vertical;
        self.settings.adjustments = parameters.adjustments;
        self.settings.background_rgb = parameters.background_rgb;
        self.settings.encoding = parameters.encoding;
        self.settings.metadata_policy = parameters.metadata;
        if let Some(worker) = &self.settings_worker {
            worker.queue(self.settings.clone());
        }
    }

    pub(crate) fn continue_with_result(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.can_cancel() || self.dialog_open || self.preview_loading {
            return;
        }
        let Some(preview) = self.preview.as_ref() else {
            return;
        };
        let format = self.image_parameters().format;
        let result_dimensions = (preview.result.width, preview.result.height);
        let source_stem = self
            .selected()
            .and_then(|asset| asset.path.file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or("image");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let directory = std::env::temp_dir().join("bunch-of-tools").join("working");
        if let Err(error) = fs::create_dir_all(&directory) {
            self.status = format!("Continue: could not create working cache: {error}");
            cx.notify();
            return;
        }
        let working_path = directory.join(format!(
            "{source_stem}-result-{nonce}.{}",
            format.extension()
        ));
        if fs::hard_link(&preview.encoded_path, &working_path)
            .or_else(|_| fs::copy(&preview.encoded_path, &working_path).map(|_| ()))
            .is_err()
        {
            self.status = "Continue: could not preserve the result".to_owned();
            cx.notify();
            return;
        }

        self.cancel_preview();
        self.remove_current_preview();
        self.assets.clear();
        self.selected_assets.clear();
        self.focused_asset = None;
        self.jobs.clear();
        self.output_directory = None;
        self.working_files.push(working_path.clone());

        self.recipe = Recipe::image_resize();
        {
            let parameters = self.image_parameters_mut();
            parameters.width = result_dimensions.0;
            parameters.height = result_dimensions.1;
            parameters.format = format;
        }
        self.recipe.output.name_template = format!("{{stem}}.optimized.{}", format.extension());
        self.dimensions_linked = true;
        self.dimensions_initialized = true;
        self.dimensions_touched = false;
        self.linked_ratio = result_dimensions;
        self.crop_enabled = false;
        self.crop_drag = None;
        self.active_tool = InspectorSection::Resize;
        self.show_result = false;
        self.fit_canvas(cx);

        self.syncing_inputs = true;
        for (input, value) in [
            (&self.width_input, result_dimensions.0),
            (&self.height_input, result_dimensions.1),
        ] {
            input.update(cx, |input, cx| {
                input.set_value(value.to_string(), window, cx)
            });
        }
        self.quality_slider.update(cx, |slider, cx| {
            slider.set_value(f32::from(self.image_parameters().quality), window, cx)
        });
        self.syncing_inputs = false;
        self.persist_parameters();
        self.add_paths([working_path]);
        self.status = "Result is now the input".to_owned();
        self.begin_preview(cx);
    }

    pub(crate) fn start_export(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.can_export() || self.dialog_open {
            return;
        }
        #[cfg(windows)]
        {
            self.dialog_open = true;
            self.status = "Choose export folder".to_owned();
            let picker = cx
                .background_executor()
                .spawn(async { run_picker_helper("--bunch-pick-folder") });
            cx.spawn(async move |this, cx| {
                let destination = picker.await;
                let _ = this.update(cx, |this, cx| {
                    this.dialog_open = false;
                    match destination {
                        Ok(mut paths) if !paths.is_empty() => {
                            let destination = paths.remove(0);
                            this.output_directory = Some(destination.clone());
                            this.submit_batch(&destination, cx);
                        }
                        Ok(_) => this.status = "Export cancelled".to_owned(),
                        Err(error) => this.status = format!("Export: {error}"),
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
        }
        #[cfg(not(windows))]
        {
            self.status = "Folder picker unavailable on this build".to_owned();
            cx.notify();
        }
    }

    fn submit_batch(&mut self, destination: &Path, cx: &mut Context<Self>) {
        let selected = self.selected_assets.iter().copied().collect::<Vec<_>>();
        self.jobs.clear();
        let mut submitted = 0_usize;
        let mut errors = Vec::new();
        for asset_index in selected {
            let Some(asset) = self.assets.get(asset_index).cloned() else {
                continue;
            };
            match self
                .plan_for(&asset, destination)
                .and_then(|plan| self.runner.submit(plan).map_err(|error| error.to_string()))
            {
                Ok(handle) => {
                    let receiver = handle.events();
                    let cancellation = handle.cancellation_token();
                    let job_id = handle.id().clone();
                    self.jobs.push(ActiveJob {
                        asset_index,
                        snapshot: JobSnapshot::new(job_id),
                        cancellation,
                    });
                    submitted += 1;
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
                Err(error) => errors.push(format!("{}: {error}", asset.name)),
            }
        }
        self.queue_open = true;
        self.status = if submitted == 0 {
            format!("Batch could not start: {}", errors.join("; "))
        } else if errors.is_empty() {
            format!("{submitted} image export(s) queued")
        } else {
            format!("{submitted} queued; {} planning error(s)", errors.len())
        };
        cx.notify();
    }

    pub(crate) fn cancel_jobs(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        for job in &self.jobs {
            if job.snapshot.state.is_some_and(|state| !state.is_terminal()) {
                job.cancellation.cancel();
            }
        }
        self.status = "Cancellation requested for active exports".to_owned();
        cx.notify();
    }

    fn plan_for(
        &self,
        asset: &AssetEntry,
        output_directory: &Path,
    ) -> Result<bunch_planner::ExecutionPlan, String> {
        let input = self
            .recipe
            .inputs
            .first()
            .ok_or_else(|| "recipe has no input slot".to_owned())?;
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
                    output_directory: output_directory.to_string_lossy().into_owned(),
                },
                &self.capabilities,
                ExecutorKind::Native,
            )
            .map_err(|error| error.to_string())
    }

    fn apply_job_event(&mut self, event: &JobEvent) {
        let Some(job) = self
            .jobs
            .iter_mut()
            .find(|job| event.job_id == job.snapshot.job_id)
        else {
            return;
        };
        match job.snapshot.apply(event) {
            Ok(()) => self.status = event_status(event, &job.snapshot.job_id),
            Err(error) => self.status = format!("Protocol: {error}"),
        }
    }

    pub(crate) fn show_source(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_result = false;
        self.fit_canvas(cx);
    }

    pub(crate) fn show_result(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_result = true;
        self.fit_canvas(cx);
    }

    pub(crate) fn fit_canvas_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fit_canvas(cx);
    }

    fn fit_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas_zoom = 1.0;
        self.canvas_pan_x = 0.0;
        self.canvas_pan_y = 0.0;
        self.canvas_drag = None;
        cx.notify();
    }

    pub(crate) fn canvas_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.modifiers.control || self.preview.is_none() {
            return;
        }
        cx.stop_propagation();
        let delta_y = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / 40.0,
            ScrollDelta::Lines(delta) => delta.y,
        };
        let old_zoom = self.canvas_zoom;
        let new_zoom = (old_zoom * 1.14_f32.powf(-delta_y)).clamp(1.0, 8.0);
        if (new_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }
        let (left, top, width, height) = self.canvas_bounds(window);
        let pointer_x = f32::from(event.position.x) - left - width / 2.0;
        let pointer_y = f32::from(event.position.y) - top - height / 2.0;
        let ratio = new_zoom / old_zoom;
        self.canvas_pan_x = pointer_x - (pointer_x - self.canvas_pan_x) * ratio;
        self.canvas_pan_y = pointer_y - (pointer_y - self.canvas_pan_y) * ratio;
        self.canvas_zoom = new_zoom;
        self.clamp_canvas_pan(window);
        cx.notify();
    }

    pub(crate) fn canvas_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left
            && self.active_tool == InspectorSection::Crop
            && self.crop_enabled
            && !self.show_result
            && let Some(point) = self.canvas_to_source(event.position, window)
            && let Some((source_width, source_height)) = self.source_dimensions()
        {
            let initial = self.image_parameters().crop.unwrap_or(CropParameters {
                x: 0,
                y: 0,
                width: source_width,
                height: source_height,
            });
            let handle = self.crop_handle_at(point, initial, window);
            let initial = if handle == CropHandle::New {
                CropParameters {
                    x: point.0.round() as u32,
                    y: point.1.round() as u32,
                    width: 1,
                    height: 1,
                }
            } else {
                initial
            };
            self.image_parameters_mut().crop = Some(initial);
            self.crop_drag = Some(CropDrag {
                handle,
                origin: point,
                initial,
            });
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if event.button == MouseButton::Left && self.canvas_zoom > 1.0 {
            self.canvas_drag = Some((event.position, self.canvas_pan_x, self.canvas_pan_y));
            cx.stop_propagation();
        }
    }

    pub(crate) fn canvas_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(drag) = self.crop_drag
            && event.dragging()
            && let Some(point) = self.canvas_to_source(event.position, window)
            && let Some((source_width, source_height)) = self.source_dimensions()
        {
            let crop = crop_from_drag(drag, point, source_width, source_height);
            self.image_parameters_mut().crop = Some(crop);
            self.status = format!("Crop {} x {}", crop.width, crop.height);
            cx.notify();
            return;
        }
        if let Some((origin, pan_x, pan_y)) = self.canvas_drag
            && event.dragging()
        {
            self.canvas_pan_x = pan_x + f32::from(event.position.x - origin.x);
            self.canvas_pan_y = pan_y + f32::from(event.position.y - origin.y);
            self.clamp_canvas_pan(window);
            cx.notify();
        }
    }

    pub(crate) fn canvas_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.crop_drag.take().is_some() {
            self.parameters_changed(cx);
            return;
        }
        if self.canvas_drag.take().is_some() {
            cx.notify();
        }
    }

    fn source_dimensions(&self) -> Option<(u32, u32)> {
        self.preview
            .as_ref()
            .map(|preview| (preview.source.width, preview.source.height))
    }

    pub(crate) fn current_crop(&self) -> Option<CropParameters> {
        self.image_parameters().crop
    }

    pub(crate) fn canvas_image_rect(&self, window: &Window) -> Option<(f32, f32, f32, f32)> {
        let (image_width, image_height) = self.canvas_image_size(window)?;
        let (left, top, canvas_width, canvas_height) = self.canvas_bounds(window);
        Some((
            left + (canvas_width - image_width) / 2.0 + self.canvas_pan_x,
            top + (canvas_height - image_height) / 2.0 + self.canvas_pan_y,
            image_width,
            image_height,
        ))
    }

    fn canvas_to_source(&self, position: Point<Pixels>, window: &Window) -> Option<(f32, f32)> {
        let (left, top, width, height) = self.canvas_image_rect(window)?;
        let (source_width, source_height) = self.source_dimensions()?;
        let position_x = f32::from(position.x);
        let position_y = f32::from(position.y);
        if position_x < left
            || position_x > left + width
            || position_y < top
            || position_y > top + height
        {
            return None;
        }
        let x = ((position_x - left) / width * source_width as f32)
            .clamp(0.0, source_width.saturating_sub(1) as f32);
        let y = ((position_y - top) / height * source_height as f32)
            .clamp(0.0, source_height.saturating_sub(1) as f32);
        Some((x, y))
    }

    fn crop_handle_at(
        &self,
        point: (f32, f32),
        crop: CropParameters,
        window: &Window,
    ) -> CropHandle {
        let Some((_, _, image_width, _)) = self.canvas_image_rect(window) else {
            return CropHandle::New;
        };
        let Some((source_width, _)) = self.source_dimensions() else {
            return CropHandle::New;
        };
        let threshold = (14.0 * source_width as f32 / image_width).max(2.0);
        let left = crop.x as f32;
        let top = crop.y as f32;
        let right = left + crop.width as f32;
        let bottom = top + crop.height as f32;
        let near =
            |x: f32, y: f32| (point.0 - x).abs() <= threshold && (point.1 - y).abs() <= threshold;
        if near(left, top) {
            CropHandle::NorthWest
        } else if near(right, top) {
            CropHandle::NorthEast
        } else if near(left, bottom) {
            CropHandle::SouthWest
        } else if near(right, bottom) {
            CropHandle::SouthEast
        } else if point.0 >= left && point.0 <= right && point.1 >= top && point.1 <= bottom {
            CropHandle::Move
        } else {
            CropHandle::New
        }
    }

    pub(crate) fn canvas_image_size(&self, window: &Window) -> Option<(f32, f32)> {
        let preview = self.preview.as_ref()?;
        let probe = if self.show_result {
            &preview.result
        } else {
            &preview.source
        };
        let (_, _, width, height) = self.canvas_bounds(window);
        let scale = (width / probe.width as f32)
            .min(height / probe.height as f32)
            .max(0.000_1);
        Some((
            probe.width as f32 * scale * self.canvas_zoom,
            probe.height as f32 * scale * self.canvas_zoom,
        ))
    }

    fn canvas_bounds(&self, window: &Window) -> (f32, f32, f32, f32) {
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);
        let left = if self.assets_open && viewport_width >= 920.0 {
            190.0
        } else {
            0.0
        };
        let right = 0.0;
        let top = 29.0 + 45.0 + 72.0 + 38.0;
        let bottom = 22.0
            + if self.inspector_open { 172.0 } else { 0.0 }
            + if self.queue_open && !self.jobs.is_empty() {
                66.0
            } else {
                0.0
            };
        (
            left,
            top,
            (viewport_width - left - right).max(160.0),
            (viewport_height - top - bottom).max(120.0),
        )
    }

    fn clamp_canvas_pan(&mut self, window: &Window) {
        let Some((image_width, image_height)) = self.canvas_image_size(window) else {
            self.canvas_pan_x = 0.0;
            self.canvas_pan_y = 0.0;
            return;
        };
        let (_, _, canvas_width, canvas_height) = self.canvas_bounds(window);
        let max_x = ((image_width - canvas_width) / 2.0).max(0.0);
        let max_y = ((image_height - canvas_height) / 2.0).max(0.0);
        self.canvas_pan_x = self.canvas_pan_x.clamp(-max_x, max_x);
        self.canvas_pan_y = self.canvas_pan_y.clamp(-max_y, max_y);
    }

    pub(crate) fn toggle_assets_panel(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.assets_open = !self.assets_open;
        cx.notify();
    }

    pub(crate) fn toggle_inspector_panel(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_open = !self.inspector_open;
        cx.notify();
    }

    pub(crate) fn toggle_queue_panel(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.queue_open = !self.queue_open;
        cx.notify();
    }

    pub(crate) fn projected_result(&self) -> Option<(u32, u32)> {
        let source = &self.preview.as_ref()?.source;
        Some(
            self.image_parameters()
                .projected_dimensions(source.width, source.height),
        )
    }
}

impl Drop for AppModel {
    fn drop(&mut self) {
        self.cancel_preview();
        self.remove_current_preview();
        for path in self.working_files.drain(..) {
            let _ = fs::remove_file(path);
        }
    }
}

fn numeric_input(
    value: u32,
    window: &mut Window,
    cx: &mut Context<AppModel>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
}

fn slider(
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    cx: &mut Context<AppModel>,
) -> Entity<SliderState> {
    cx.new(|_| {
        SliderState::new()
            .min(min)
            .max(max)
            .step(step)
            .default_value(value)
    })
}

fn image_parameters(recipe: &Recipe) -> &ImageResizeParameters {
    match &recipe.steps[0].operation {
        Operation::ImageResize(parameters) => parameters,
        Operation::FoundationSimulate(_) => unreachable!("desktop uses image recipe"),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum DimensionUpdate {
    Unchanged,
    Changed { linked_value: Option<u32> },
}

fn apply_dimension_input(
    parameters: &mut ImageResizeParameters,
    is_width: bool,
    value: u32,
    dimensions_linked: bool,
    source_width: u32,
    source_height: u32,
) -> DimensionUpdate {
    let current = if is_width {
        parameters.width
    } else {
        parameters.height
    };
    if current == value {
        return DimensionUpdate::Unchanged;
    }

    if is_width {
        parameters.width = value;
    } else {
        parameters.height = value;
    }

    if !dimensions_linked {
        return DimensionUpdate::Changed { linked_value: None };
    }

    let linked = if is_width {
        f64::from(value) * f64::from(source_height.max(1)) / f64::from(source_width.max(1))
    } else {
        f64::from(value) * f64::from(source_width.max(1)) / f64::from(source_height.max(1))
    }
    .round()
    .clamp(1.0, 8_192.0) as u32;

    if is_width {
        parameters.height = linked;
    } else {
        parameters.width = linked;
    }

    DimensionUpdate::Changed {
        linked_value: Some(linked),
    }
}

fn normalize_rotation(rotation: i16) -> i16 {
    (rotation + 180).rem_euclid(360) - 180
}

fn crop_from_drag(
    drag: CropDrag,
    point: (f32, f32),
    source_width: u32,
    source_height: u32,
) -> CropParameters {
    let max_x = source_width.max(1) as f32;
    let max_y = source_height.max(1) as f32;
    if drag.handle == CropHandle::Move {
        let dx = point.0 - drag.origin.0;
        let dy = point.1 - drag.origin.1;
        return CropParameters {
            x: (drag.initial.x as f32 + dx)
                .round()
                .clamp(0.0, max_x - drag.initial.width as f32) as u32,
            y: (drag.initial.y as f32 + dy)
                .round()
                .clamp(0.0, max_y - drag.initial.height as f32) as u32,
            ..drag.initial
        };
    }

    let mut left = drag.initial.x as f32;
    let mut top = drag.initial.y as f32;
    let mut right = left + drag.initial.width as f32;
    let mut bottom = top + drag.initial.height as f32;
    match drag.handle {
        CropHandle::New => {
            left = drag.origin.0.min(point.0);
            top = drag.origin.1.min(point.1);
            right = drag.origin.0.max(point.0);
            bottom = drag.origin.1.max(point.1);
        }
        CropHandle::NorthWest => {
            left = point.0.min(right - 1.0);
            top = point.1.min(bottom - 1.0);
        }
        CropHandle::NorthEast => {
            right = point.0.max(left + 1.0);
            top = point.1.min(bottom - 1.0);
        }
        CropHandle::SouthWest => {
            left = point.0.min(right - 1.0);
            bottom = point.1.max(top + 1.0);
        }
        CropHandle::SouthEast => {
            right = point.0.max(left + 1.0);
            bottom = point.1.max(top + 1.0);
        }
        CropHandle::Move => unreachable!(),
    }
    left = left.clamp(0.0, max_x - 1.0);
    top = top.clamp(0.0, max_y - 1.0);
    right = right.clamp(left + 1.0, max_x);
    bottom = bottom.clamp(top + 1.0, max_y);
    let x = left.floor() as u32;
    let y = top.floor() as u32;
    let right = right.ceil() as u32;
    let bottom = bottom.ceil() as u32;
    CropParameters {
        x,
        y,
        width: right.saturating_sub(x).max(1),
        height: bottom.saturating_sub(y).max(1),
    }
}

fn remove_preview_files(preview: &ImagePreview, preserve_source: Option<&Path>) {
    if preserve_source != Some(preview.source_path.as_path()) {
        let _ = fs::remove_file(&preview.source_path);
    }
    let _ = fs::remove_file(&preview.encoded_path);
    let _ = fs::remove_file(&preview.result_path);
}

fn decode_preview_texture(path: &Path) -> Result<Arc<RenderImage>, String> {
    let mut pixels = image::open(path)
        .map_err(|error| format!("could not decode preview texture: {error}"))?
        .into_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(smallvec![Frame::new(pixels)])))
}

#[cfg(windows)]
fn run_picker_helper(mode: &str) -> Result<Vec<PathBuf>, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the application: {error}"))?;
    let output = Command::new(executable)
        .arg(mode)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not open the native picker: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("native picker returned invalid data: {error}"))
}

fn apply_settings(recipe: &mut Recipe, settings: &DesktopSettings) {
    let Operation::ImageResize(parameters) = &mut recipe.steps[0].operation else {
        return;
    };
    parameters.width = settings.image_width;
    parameters.height = settings.image_height;
    parameters.format = settings.image_format;
    parameters.quality = settings.image_quality;
    parameters.mode = settings.resize_mode;
    parameters.allow_upscale = settings.allow_upscale;
    parameters.pad_to_size = settings.pad_to_size;
    parameters.rotation_degrees = settings.rotation_degrees;
    parameters.flip_horizontal = settings.flip_horizontal;
    parameters.flip_vertical = settings.flip_vertical;
    parameters.adjustments = settings.adjustments;
    parameters.background_rgb = settings.background_rgb;
    parameters.encoding = EncodingMode::Automatic;
    parameters.metadata = settings.metadata_policy;
    recipe.output.name_template =
        format!("{{stem}}.optimized.{}", settings.image_format.extension());
}

fn event_status(event: &JobEvent, job_id: &JobId) -> String {
    event.terminal_state().map_or_else(
        || format!("{} / event {}", job_id.0, event.sequence),
        |state| format!("{} / {state:?}", job_id.0),
    )
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn run() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        let component_theme = gpui_component::Theme::global_mut(cx);
        component_theme.radius = px(0.0);
        component_theme.radius_lg = px(0.0);
        component_theme.font_family = UI_FONT.into();
        component_theme.mono_font_family = MONO_FONT.into();

        let bounds = Bounds::centered(None, size(px(1320.0), px(860.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(820.0), px(560.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Bunch of Tools".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("bunch-of-tools".to_owned()),
                ..WindowOptions::default()
            },
            |window, cx| {
                let model = cx.new(|cx| AppModel::new(window, cx));
                cx.new(|cx| gpui_component::Root::new(model, window, cx))
            },
        )
        .expect("Bunch of Tools window must open");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use bunch_core::{CropParameters, Recipe};

    use crate::settings::DesktopSettings;

    use super::{
        CropDrag, CropHandle, DimensionUpdate, apply_dimension_input, apply_settings,
        crop_from_drag, image_parameters, normalize_rotation,
    };

    #[test]
    fn desktop_defaults_use_the_shared_image_recipe() {
        let settings = DesktopSettings::default();
        let mut recipe = Recipe::image_resize();
        let expected = recipe.clone();
        apply_settings(&mut recipe, &settings);
        assert_eq!(recipe, expected);
    }

    #[test]
    fn linked_dimension_programmatic_change_is_idempotent() {
        let mut recipe = Recipe::image_resize();
        let parameters = match &mut recipe.steps[0].operation {
            bunch_core::Operation::ImageResize(parameters) => parameters,
            bunch_core::Operation::FoundationSimulate(_) => unreachable!(),
        };

        let update = apply_dimension_input(parameters, true, 1000, true, 804, 942);
        assert_eq!(
            update,
            DimensionUpdate::Changed {
                linked_value: Some(1172)
            }
        );

        let queued_counterpart = apply_dimension_input(parameters, false, 1172, true, 804, 942);
        assert_eq!(queued_counterpart, DimensionUpdate::Unchanged);
        assert_eq!((parameters.width, parameters.height), (1000, 1172));
        assert_eq!(image_parameters(&recipe).width, 1000);
    }

    #[test]
    fn crop_drag_normalizes_reverse_selection_and_stays_in_source() {
        let crop = crop_from_drag(
            CropDrag {
                handle: CropHandle::New,
                origin: (900.0, 700.0),
                initial: CropParameters {
                    x: 900,
                    y: 700,
                    width: 1,
                    height: 1,
                },
            },
            (100.0, 50.0),
            1_000,
            800,
        );
        assert_eq!(
            crop,
            CropParameters {
                x: 100,
                y: 50,
                width: 800,
                height: 650,
            }
        );
    }

    #[test]
    fn crop_move_clamps_without_resizing() {
        let initial = CropParameters {
            x: 100,
            y: 100,
            width: 400,
            height: 300,
        };
        let crop = crop_from_drag(
            CropDrag {
                handle: CropHandle::Move,
                origin: (200.0, 200.0),
                initial,
            },
            (999.0, 799.0),
            1_000,
            800,
        );
        assert_eq!((crop.x, crop.y), (600, 500));
        assert_eq!((crop.width, crop.height), (400, 300));
    }

    #[test]
    fn quarter_turns_remain_inside_recipe_rotation_range() {
        assert_eq!(normalize_rotation(270), -90);
        assert_eq!(normalize_rotation(-270), 90);
        assert_eq!(normalize_rotation(180), -180);
    }
}

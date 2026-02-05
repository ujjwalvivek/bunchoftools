use bunch_native::EngineStatus;
use bunch_protocol::{JobSnapshot, JobState};
use gpui::prelude::*;
use gpui::*;

use crate::app::AppModel;
use crate::theme::*;

impl Render for AppModel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .min_w(px(900.0))
            .min_h(px(620.0))
            .bg(rgb(BACKGROUND))
            .text_color(rgb(TEXT))
            .font_family(UI_FONT)
            .flex()
            .flex_col()
            .child(self.app_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(self.assets_panel(cx))
                    .child(self.preview_panel())
                    .child(self.inspector_panel(cx)),
            )
            .child(self.queue_panel(cx))
            .child(self.status_bar())
    }
}

impl AppModel {
    fn app_bar(&self, cx: &mut Context<Self>) -> Div {
        let can_run = self.can_run();
        let can_cancel = self.can_cancel();
        div()
            .h(px(45.0))
            .flex_none()
            .px(px(12.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(216.0))
                    .flex_none()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(14.0))
                            .child("BUNCH OF TOOLS"),
                    )
                    .child(micro("M1 / NATIVE WORKBENCH", DIM)),
            )
            .child(
                command_button("open-files", "OPEN", true, ACCENT)
                    .on_click(cx.listener(Self::open_files)),
            )
            .child(
                command_button("clear-files", "CLEAR", !self.assets.is_empty(), MUTED)
                    .when(!self.assets.is_empty(), |button| {
                        button.on_click(cx.listener(Self::clear_assets))
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(px(9.0))
                    .text_color(rgb(DIM))
                    .child("FOUNDATION.SIMULATE"),
            )
            .child(
                command_button("cancel-job", "CANCEL", can_cancel, ERROR)
                    .when(can_cancel, |button| {
                        button.on_click(cx.listener(Self::cancel_job))
                    }),
            )
            .child(
                command_button("run-job", "RUN", can_run, SUCCESS).when(can_run, |button| {
                    button.on_click(cx.listener(Self::start_job))
                }),
            )
    }

    fn assets_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div()
            .id("asset-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll();
        if self.assets.is_empty() {
            list = list.child(empty_message("NO ASSETS"));
        } else {
            for (index, asset) in self.assets.iter().enumerate() {
                let selected = self.selected_asset == Some(index);
                let name = asset.name.clone();
                let path = asset.path.to_string_lossy().into_owned();
                list = list.child(
                    div()
                        .id(("asset-row", index))
                        .h(px(49.0))
                        .px(px(10.0))
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(if selected { ACCENT_DARK } else { SURFACE }))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(HOVER)))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(11.0))
                                .text_color(rgb(if selected { TEXT } else { MUTED }))
                                .child(name),
                        )
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(MONO_FONT)
                                .text_size(px(8.0))
                                .text_color(rgb(DIM))
                                .child(path),
                        )
                        .on_click(cx.listener(move |this, event, window, cx| {
                            this.select_asset(index, event, window, cx);
                        })),
                );
            }
        }

        panel("ASSETS", "INPUT BINDINGS")
            .w(px(240.0))
            .flex_none()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(list)
    }

    fn preview_panel(&self) -> Div {
        let content = self.selected().map_or_else(
            || empty_message("SELECT AN ASSET"),
            |asset| {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(88.0))
                            .h(px(68.0))
                            .border_1()
                            .border_color(rgb(BORDER_STRONG))
                            .bg(rgb(RAISED))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(MONO_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(11.0))
                            .text_color(rgb(ACCENT))
                            .child("ASSET"),
                    )
                    .child(
                        div()
                            .mt(px(15.0))
                            .max_w(px(520.0))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(14.0))
                            .child(asset.name.clone()),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .max_w(px(560.0))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_family(MONO_FONT)
                            .text_size(px(9.0))
                            .text_color(rgb(DIM))
                            .child(asset.path.to_string_lossy().into_owned()),
                    )
            },
        );

        panel("PREVIEW", "SELECTED ASSET")
            .flex_1()
            .min_w(px(0.0))
            .child(content)
    }

    fn inspector_panel(&self, cx: &mut Context<Self>) -> Div {
        let duration = self.duration_ms();
        let operation = div()
            .p(px(12.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(section_label("OPERATION"))
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(px(12.0))
                    .child("Foundation simulation"),
            )
            .child(
                div()
                    .mt(px(3.0))
                    .font_family(MONO_FONT)
                    .text_size(px(9.0))
                    .text_color(rgb(DIM))
                    .child("foundation.simulate / schema 1"),
            );

        let duration_controls = div()
            .p(px(12.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(section_label("DURATION"))
            .child(
                div()
                    .mt(px(8.0))
                    .flex()
                    .gap(px(5.0))
                    .child(
                        segment("duration-fast", "FAST", duration == 500)
                            .on_click(cx.listener(Self::duration_fast)),
                    )
                    .child(
                        segment("duration-normal", "NORMAL", duration == 1_200)
                            .on_click(cx.listener(Self::duration_normal)),
                    )
                    .child(
                        segment("duration-slow", "SLOW", duration == 3_000)
                            .on_click(cx.listener(Self::duration_slow)),
                    ),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .font_family(MONO_FONT)
                    .text_size(px(9.0))
                    .text_color(rgb(MUTED))
                    .child(format!("{duration} MS")),
            );

        let diagnostics = self.diagnostics.iter().fold(
            div().p(px(12.0)).child(section_label("EXECUTOR")),
            |list, diagnostic| {
                let ready = diagnostic.status == EngineStatus::Ready;
                list.child(
                    div()
                        .mt(px(8.0))
                        .flex()
                        .items_start()
                        .gap(px(7.0))
                        .child(div().mt(px(4.0)).size(px(6.0)).bg(rgb(if ready {
                            SUCCESS
                        } else {
                            ERROR
                        })))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .child(diagnostic.engine_name.clone()),
                                )
                                .child(
                                    div()
                                        .mt(px(2.0))
                                        .font_family(MONO_FONT)
                                        .text_size(px(8.0))
                                        .text_color(rgb(DIM))
                                        .child(format!(
                                            "{} / {}",
                                            diagnostic.adapter_id, diagnostic.engine_version
                                        )),
                                ),
                        ),
                )
            },
        );

        panel("INSPECTOR", "RECIPE PARAMETERS")
            .w(px(284.0))
            .flex_none()
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(operation)
            .child(duration_controls)
            .child(diagnostics)
    }

    fn queue_panel(&self, cx: &mut Context<Self>) -> Div {
        let content = self.active_job.as_ref().map_or_else(
            || empty_message("QUEUE EMPTY"),
            |job| job_row(&job.snapshot, self.can_cancel(), cx),
        );
        panel("QUEUE", "JOB EVENTS")
            .h(px(142.0))
            .flex_none()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(content)
    }

    fn status_bar(&self) -> Div {
        div()
            .h(px(24.0))
            .flex_none()
            .px(px(10.0))
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .flex()
            .items_center()
            .child(micro(self.status.clone(), MUTED))
            .child(div().flex_1())
            .child(micro(
                format!("{} CAPABILITY", self.capabilities.all().len()),
                DIM,
            ))
            .child(div().mx(px(8.0)).h(px(11.0)).w(px(1.0)).bg(rgb(BORDER)))
            .child(micro("LOCAL / NO FILE WRITES", SUCCESS))
    }
}

fn job_row(snapshot: &JobSnapshot, can_cancel: bool, cx: &mut Context<AppModel>) -> Div {
    let state = snapshot.state.unwrap_or(JobState::Queued);
    let color = state_color(state);
    let width = if snapshot.progress_total == 0 {
        0.0
    } else {
        360.0 * snapshot.progress_completed as f32 / snapshot.progress_total as f32
    };
    div()
        .h_full()
        .px(px(12.0))
        .py(px(10.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .child(
            div()
                .w(px(145.0))
                .flex_none()
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(px(10.0))
                        .text_color(rgb(color))
                        .child(format!("{:?}", state).to_ascii_uppercase()),
                )
                .child(
                    div()
                        .mt(px(3.0))
                        .font_family(MONO_FONT)
                        .text_size(px(8.0))
                        .text_color(rgb(DIM))
                        .child(snapshot.job_id.0.to_ascii_uppercase()),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(10.0))
                                .text_color(rgb(MUTED))
                                .child(snapshot.progress_message.clone()),
                        )
                        .child(div().flex_1())
                        .child(micro(
                            format!(
                                "{} / {}",
                                snapshot.progress_completed, snapshot.progress_total
                            ),
                            DIM,
                        )),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .h(px(5.0))
                        .w(px(360.0))
                        .bg(rgb(RAISED))
                        .child(div().h_full().w(px(width)).bg(rgb(color))),
                )
                .child(
                    div()
                        .mt(px(7.0))
                        .font_family(MONO_FONT)
                        .text_size(px(8.0))
                        .text_color(rgb(DIM))
                        .child(format!(
                            "{} ARTIFACT(S) / {} DIAGNOSTIC(S) / EVENT {}",
                            snapshot.artifacts.len(),
                            snapshot.diagnostics.len(),
                            snapshot.last_sequence
                        )),
                ),
        )
        .child(
            command_button("queue-cancel", "CANCEL", can_cancel, ERROR)
                .when(can_cancel, |button| {
                    button.on_click(cx.listener(AppModel::cancel_job))
                }),
        )
}

fn panel(title: &'static str, subtitle: &'static str) -> Div {
    div()
        .h_full()
        .min_h(px(0.0))
        .bg(rgb(BACKGROUND))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(31.0))
                .flex_none()
                .px(px(10.0))
                .border_b_1()
                .border_color(rgb(BORDER))
                .bg(rgb(SURFACE))
                .flex()
                .items_center()
                .child(section_label(title))
                .child(div().flex_1())
                .child(micro(subtitle, DIM)),
        )
}

fn section_label(label: impl Into<SharedString>) -> Div {
    div()
        .font_family(MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(9.0))
        .text_color(rgb(MUTED))
        .child(label.into())
}

fn micro(label: impl Into<SharedString>, color: u32) -> Div {
    div()
        .font_family(MONO_FONT)
        .text_size(px(8.0))
        .text_color(rgb(color))
        .child(label.into())
}

fn empty_message(label: &'static str) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .text_size(px(9.0))
        .text_color(rgb(DIM))
        .child(label)
}

fn command_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    active_color: u32,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(25.0))
        .min_w(px(58.0))
        .px(px(9.0))
        .border_1()
        .border_color(rgb(if enabled { active_color } else { BORDER }))
        .bg(rgb(if enabled { RAISED } else { SURFACE }))
        .text_color(rgb(if enabled { active_color } else { DIM }))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(9.0))
        .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .active(|style| style.bg(rgb(ACCENT_DARK)))
        .child(label)
}

fn segment(id: &'static str, label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.0))
        .px(px(7.0))
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active { ACCENT_DARK } else { RAISED }))
        .text_color(rgb(if active { ACCENT } else { MUTED }))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .text_size(px(8.0))
        .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(label)
}

const fn state_color(state: JobState) -> u32 {
    match state {
        JobState::Succeeded => SUCCESS,
        JobState::Failed => ERROR,
        JobState::Cancelled | JobState::Cancelling => WARNING,
        JobState::Queued | JobState::Preparing | JobState::Running | JobState::Finalizing => ACCENT,
    }
}

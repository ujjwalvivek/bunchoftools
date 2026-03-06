use bunch_core::{ImageFormat, MetadataPolicy};
use bunch_native::EngineStatus;
use bunch_protocol::{JobSnapshot, JobState};
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::Button;
use gpui_component::input::Input;
use gpui_component::slider::Slider;
use gpui_component::{Icon, IconName, Selectable};

use crate::app::{AppModel, InspectorSection, format_bytes};
use crate::theme::*;

const TITLEBAR_HEIGHT: f32 = 29.0;
const ACTION_HEIGHT: f32 = 45.0;
const STATS_HEIGHT: f32 = 72.0;
const ASSET_RAIL_WIDTH: f32 = 190.0;
const CANVAS_TOOLBAR_HEIGHT: f32 = 38.0;
const DECK_HEIGHT: f32 = 172.0;
const STATUS_HEIGHT: f32 = 22.0;

impl Render for AppModel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_pending_dimension_inputs(window, cx);
        let show_assets = self.assets_open && f32::from(window.viewport_size().width) >= 920.0;

        div()
            .id("app-root")
            .size_full()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(rgb(APP_BACKGROUND))
            .text_color(rgb(TEXT))
            .font_family(UI_FONT)
            .text_size(px(13.0))
            .flex()
            .flex_col()
            .on_drop(cx.listener(Self::drop_files))
            .child(self.titlebar(window, cx))
            .child(self.action_bar(cx))
            .child(self.stats_rail())
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .min_w(px(0.0))
                    .flex()
                    .when(show_assets, |row| row.child(self.asset_rail(cx)))
                    .child(self.canvas_stage(window, cx)),
            )
            .when(self.inspector_open, |column| {
                column.child(self.operation_deck(cx))
            })
            .when(self.queue_open && !self.jobs.is_empty(), |column| {
                column.child(self.queue_strip())
            })
            .child(self.status_bar())
    }
}

impl AppModel {
    fn titlebar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let maximized = window.is_maximized();
        let context = self
            .selected()
            .map_or_else(String::new, |asset| asset.name.clone());

        div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .border_b_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(TITLEBAR))
            .flex()
            .items_center()
            .child(div().ml(px(10.0)).w(px(3.0)).h(px(14.0)).bg(rgb(ACCENT)))
            .child(
                div()
                    .id("window-drag-region")
                    .h_full()
                    .min_w(px(0.0))
                    .flex_1()
                    .px(px(9.0))
                    .gap(px(10.0))
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |event, window, _| {
                        if event.click_count == 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.0))
                            .text_color(rgb(TEXT))
                            .child("BUNCH"),
                    )
                    .when(!context.is_empty(), |drag| {
                        drag.child(
                            div()
                                .min_w(px(0.0))
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(MONO_FONT)
                                .text_size(px(11.0))
                                .text_color(rgb(DIM))
                                .child(context),
                        )
                    }),
            )
            .child(
                title_icon_button(
                    "toggle-assets",
                    if self.assets_open {
                        IconName::PanelLeftClose
                    } else {
                        IconName::PanelLeftOpen
                    },
                    "Assets",
                    self.assets_open,
                )
                .on_click(cx.listener(Self::toggle_assets_panel)),
            )
            .child(
                title_icon_button(
                    "toggle-tools",
                    if self.inspector_open {
                        IconName::PanelBottom
                    } else {
                        IconName::PanelBottomOpen
                    },
                    "Tools",
                    self.inspector_open,
                )
                .on_click(cx.listener(Self::toggle_inspector_panel)),
            )
            .when(!self.jobs.is_empty(), |bar| {
                bar.child(
                    title_icon_button(
                        "toggle-jobs",
                        IconName::GalleryVerticalEnd,
                        "Jobs",
                        self.queue_open,
                    )
                    .on_click(cx.listener(Self::toggle_queue_panel)),
                )
            })
            .child(window_control(
                "window-minimize",
                IconName::WindowMinimize,
                WindowControlArea::Min,
                |window| window.minimize_window(),
            ))
            .child(window_control(
                "window-maximize",
                if maximized {
                    IconName::WindowRestore
                } else {
                    IconName::WindowMaximize
                },
                WindowControlArea::Max,
                |window| window.zoom_window(),
            ))
            .child(window_control(
                "window-close",
                IconName::WindowClose,
                WindowControlArea::Close,
                |window| window.remove_window(),
            ))
    }

    fn action_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(ACTION_HEIGHT))
            .flex_none()
            .px(px(10.0))
            .border_b_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(APP_BACKGROUND))
            .flex()
            .items_center()
            .gap(px(7.0))
            .child(
                command_button(
                    "import",
                    IconName::FolderOpen,
                    "Import",
                    !self.dialog_open && !self.can_cancel(),
                    false,
                )
                .when(!self.dialog_open && !self.can_cancel(), |button| {
                    button.on_click(cx.listener(Self::open_files))
                }),
            )
            .when(!self.assets.is_empty(), |bar| {
                bar.child(
                    div()
                        .ml(px(4.0))
                        .font_family(MONO_FONT)
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(format!(
                            "{} selected / {}",
                            self.selected_assets.len(),
                            self.assets.len()
                        )),
                )
                .child(
                    quiet_button("select-all", "All", true).on_click(cx.listener(Self::select_all)),
                )
                .child(
                    quiet_button("select-none", "None", true)
                        .on_click(cx.listener(Self::select_none)),
                )
                .child(
                    quiet_button("clear-assets", "Clear", !self.can_cancel())
                        .when(!self.can_cancel(), |button| {
                            button.on_click(cx.listener(Self::clear_assets))
                        }),
                )
            })
            .child(div().flex_1())
            .when(self.can_cancel(), |bar| {
                bar.child(
                    command_button("cancel", IconName::CircleX, "Cancel", true, true)
                        .on_click(cx.listener(Self::cancel_jobs)),
                )
            })
            .when(
                self.preview.is_some()
                    && !self.preview_loading
                    && !self.can_cancel()
                    && !self.dialog_open,
                |bar| {
                    bar.child(
                        command_button(
                            "continue-result",
                            IconName::Replace,
                            "Continue",
                            true,
                            false,
                        )
                        .on_click(cx.listener(Self::continue_with_result)),
                    )
                },
            )
            .child(
                command_button(
                    "export",
                    IconName::Folder,
                    "Export",
                    self.can_export() && !self.dialog_open,
                    false,
                )
                .when(self.can_export() && !self.dialog_open, |button| {
                    button.on_click(cx.listener(Self::start_export))
                }),
            )
    }

    fn stats_rail(&self) -> Div {
        let (source_value, source_detail) = self.preview.as_ref().map_or_else(
            || ("--".to_owned(), "NO SOURCE".to_owned()),
            |preview| {
                (
                    format!("{} x {}", preview.source.width, preview.source.height),
                    format!(
                        "{}  {}",
                        preview.source.format,
                        format_bytes(preview.source.file_size)
                    ),
                )
            },
        );
        let projected = self.projected_result();
        let result_value = projected.map_or_else(
            || "--".to_owned(),
            |(width, height)| format!("{width} x {height}"),
        );
        let parameters = self.image_parameters();
        let result_detail = self.preview.as_ref().map_or_else(
            || parameters.format.label().to_owned(),
            |preview| {
                format!(
                    "{}  {}",
                    parameters.format.label(),
                    format_bytes(preview.result.file_size)
                )
            },
        );
        let (change_value, change_detail, change_color) = self.preview.as_ref().map_or_else(
            || ("--".to_owned(), "ENCODED SIZE".to_owned(), MUTED),
            |preview| {
                let source = preview.source.file_size.max(1);
                let result = preview.result.file_size;
                let change = (result as f64 - source as f64) * 100.0 / source as f64;
                let delta = source.abs_diff(result);
                (
                    format!("{change:+.1}%"),
                    format!(
                        "{} {}",
                        format_bytes(delta),
                        if result <= source { "saved" } else { "added" }
                    ),
                    if result <= source { SUCCESS } else { WARNING },
                )
            },
        );
        let batch_value = format!("{} / {}", self.selected_assets.len(), self.assets.len());
        let batch_detail = if self.jobs.is_empty() {
            "BATCH".to_owned()
        } else {
            format!("{} JOBS", self.jobs.len())
        };

        div()
            .h(px(STATS_HEIGHT))
            .flex_none()
            .border_b_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(STATS_BACKGROUND))
            .flex()
            .child(stat_block("SOURCE", source_value, source_detail, TEXT))
            .child(stat_block("RESULT", result_value, result_detail, ACCENT))
            .child(stat_block(
                "CHANGE",
                change_value,
                change_detail,
                change_color,
            ))
            .child(stat_block("READY", batch_value, batch_detail, TEXT))
    }

    fn asset_rail(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div()
            .id("asset-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .scrollbar_width(px(4.0));

        for (index, asset) in self.assets.iter().enumerate() {
            let focused = self.focused_asset == Some(index);
            let selected = self.selected_assets.contains(&index);
            let name = asset.name.clone();
            let extension = asset
                .path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("IMG")
                .to_ascii_uppercase();
            list = list.child(
                div()
                    .id(("asset-row", index))
                    .relative()
                    .h(px(54.0))
                    .flex_none()
                    .border_b_1()
                    .border_color(rgb(BORDER_SOFT))
                    .bg(rgb(if focused { ACTIVE_ROW } else { APP_BACKGROUND }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(HOVER)))
                    .flex()
                    .items_center()
                    .when(focused, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.0))
                                .bg(rgb(ACCENT)),
                        )
                    })
                    .child(
                        div()
                            .id(("asset-check", index))
                            .w(px(34.0))
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(check_box(selected))
                            .on_click(cx.listener(move |this, event, window, cx| {
                                this.toggle_asset_selection(index, event, window, cx);
                            })),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .pr(px(7.0))
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .font_family(MONO_FONT)
                                    .text_size(px(11.0))
                                    .text_color(rgb(if focused { TEXT } else { MUTED }))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .font_family(MONO_FONT)
                                    .text_size(px(9.0))
                                    .text_color(rgb(DIM))
                                    .child(extension),
                            ),
                    )
                    .on_click(cx.listener(move |this, event, window, cx| {
                        this.focus_asset(index, event, window, cx);
                    })),
            );
        }

        div()
            .w(px(ASSET_RAIL_WIDTH))
            .flex_none()
            .min_w(px(0.0))
            .border_r_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(APP_BACKGROUND))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.0))
                    .flex_none()
                    .px(px(9.0))
                    .border_b_1()
                    .border_color(rgb(BORDER_SOFT))
                    .flex()
                    .items_center()
                    .font_family(MONO_FONT)
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child("ASSETS")
                    .child(div().flex_1())
                    .child(self.assets.len().to_string()),
            )
            .child(list)
    }

    fn canvas_stage(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let toolbar = div()
            .h(px(CANVAS_TOOLBAR_HEIGHT))
            .flex_none()
            .px(px(8.0))
            .border_b_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(CANVAS_TOOLBAR))
            .flex()
            .items_center()
            .gap(px(5.0))
            .child(
                segment("view-source", "Source", !self.show_result)
                    .on_click(cx.listener(Self::show_source)),
            )
            .child(
                segment("view-result", "Result", self.show_result)
                    .on_click(cx.listener(Self::show_result)),
            )
            .when(self.preview_loading, |bar| {
                bar.child(div().ml(px(5.0)).size(px(6.0)).bg(rgb(ACCENT)))
            })
            .child(div().flex_1())
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(format!("{:.0}%", self.canvas_zoom * 100.0)),
            )
            .child(
                title_icon_button("fit-canvas", IconName::Minimize, "Fit", false)
                    .on_click(cx.listener(Self::fit_canvas_click)),
            );

        let texture = if self.show_result {
            self.preview_result_texture.clone()
        } else {
            self.preview_source_texture.clone()
        };
        let canvas = if let Some(texture) = texture {
            let (image_width, image_height) =
                self.canvas_image_size(window).unwrap_or((1.0_f32, 1.0_f32));
            let image = div()
                .relative()
                .left(px(self.canvas_pan_x))
                .top(px(self.canvas_pan_y))
                .w(px(image_width))
                .h(px(image_height))
                .flex_none()
                .child(img(texture).size_full().object_fit(ObjectFit::Contain))
                .when(
                    self.active_tool == InspectorSection::Crop
                        && self.crop_enabled
                        && !self.show_result,
                    |image| image.child(self.crop_overlay(image_width, image_height)),
                );
            div()
                .id("image-canvas")
                .relative()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .overflow_hidden()
                .bg(rgb(CANVAS))
                .cursor_pointer()
                .on_scroll_wheel(cx.listener(Self::canvas_scroll))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::canvas_mouse_down))
                .on_mouse_move(cx.listener(Self::canvas_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::canvas_mouse_up))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(image),
                )
                .when_some(self.preview_error.clone(), |canvas, error| {
                    canvas.child(error_banner(error))
                })
                .into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h(px(0.0))
                .bg(rgb(CANVAS))
                .child(empty_message(if self.preview_loading {
                    "Rendering"
                } else {
                    "Drop in an image"
                }))
                .when_some(self.preview_error.clone(), |canvas, error| {
                    canvas.child(error_banner(error))
                })
                .into_any_element()
        };

        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .bg(rgb(CANVAS))
            .flex()
            .flex_col()
            .child(toolbar)
            .child(canvas)
    }

    fn crop_overlay(&self, image_width: f32, image_height: f32) -> Div {
        let Some(preview) = &self.preview else {
            return div();
        };
        let crop = self.current_crop().unwrap_or(bunch_core::CropParameters {
            x: 0,
            y: 0,
            width: preview.source.width,
            height: preview.source.height,
        });
        let left = crop.x as f32 / preview.source.width as f32 * image_width;
        let top = crop.y as f32 / preview.source.height as f32 * image_height;
        let width = crop.width as f32 / preview.source.width as f32 * image_width;
        let height = crop.height as f32 / preview.source.height as f32 * image_height;
        let right = left + width;
        let bottom = top + height;
        let shade = rgba(0x000000a6);
        let guide = rgba(0xffffff38);

        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(top))
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(bottom))
                    .w_full()
                    .bottom_0()
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(top))
                    .w(px(left))
                    .h(px(height))
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .left(px(right))
                    .right_0()
                    .top(px(top))
                    .h(px(height))
                    .bg(shade),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .border_1()
                    .border_color(rgb(TEXT))
                    .child(
                        div()
                            .absolute()
                            .left(px(width / 3.0))
                            .top_0()
                            .bottom_0()
                            .w(px(1.0))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(width * 2.0 / 3.0))
                            .top_0()
                            .bottom_0()
                            .w(px(1.0))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(height / 3.0))
                            .h(px(1.0))
                            .bg(guide),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(height * 2.0 / 3.0))
                            .h(px(1.0))
                            .bg(guide),
                    )
                    .children([
                        crop_handle(-5.0, -5.0),
                        crop_handle(width - 5.0, -5.0),
                        crop_handle(-5.0, height - 5.0),
                        crop_handle(width - 5.0, height - 5.0),
                    ]),
            )
    }

    fn operation_deck(&self, cx: &mut Context<Self>) -> Div {
        let body = match self.active_tool {
            InspectorSection::Resize => self.transform_tools(cx),
            InspectorSection::Crop => self.crop_tools(cx),
            InspectorSection::Encode => self.encode_tools(cx),
        };

        div()
            .h(px(DECK_HEIGHT))
            .flex_none()
            .border_t_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(DECK_BACKGROUND))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(39.0))
                    .flex_none()
                    .px(px(10.0))
                    .border_b_1()
                    .border_color(rgb(BORDER_SOFT))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        tool_tab(
                            "tool-transform",
                            "RESIZE",
                            self.active_tool == InspectorSection::Resize,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_active_tool(InspectorSection::Resize, cx);
                        })),
                    )
                    .child(
                        tool_tab(
                            "tool-crop",
                            "CROP",
                            self.active_tool == InspectorSection::Crop,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_active_tool(InspectorSection::Crop, cx);
                        })),
                    )
                    .child(
                        tool_tab(
                            "tool-output",
                            "CONVERT",
                            self.active_tool == InspectorSection::Encode,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_active_tool(InspectorSection::Encode, cx);
                        })),
                    )
                    .child(div().flex_1())
                    .when_some(self.selected(), |bar, asset| {
                        bar.child(
                            div()
                                .max_w(px(320.0))
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(MONO_FONT)
                                .text_size(px(10.0))
                                .text_color(rgb(DIM))
                                .child(asset.name.clone()),
                        )
                    }),
            )
            .child(body)
    }

    fn transform_tools(&self, cx: &mut Context<Self>) -> Div {
        let parameters = self.image_parameters();
        deck_body()
            .child(
                deck_group("SIZE", 390.0)
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .items_end()
                            .gap(px(7.0))
                            .child(deck_input(&self.width_input, "W", "PX"))
                            .child(
                                square_toggle(
                                    "link-dimensions",
                                    IconName::ExternalLink,
                                    self.dimensions_linked,
                                    "Lock ratio",
                                )
                                .on_click(cx.listener(Self::toggle_dimensions_link)),
                            )
                            .child(deck_input(&self.height_input, "H", "PX")),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .flex()
                            .items_center()
                            .font_family(MONO_FONT)
                            .text_size(px(10.0))
                            .text_color(rgb(MUTED))
                            .child(if self.dimensions_linked {
                                "PROPORTIONS LOCKED"
                            } else {
                                "EXACT SIZE"
                            }),
                    ),
            )
            .child(
                deck_group("ORIENT", 300.0).child(
                    div()
                        .mt(px(10.0))
                        .flex()
                        .gap(px(6.0))
                        .child(
                            square_toggle("rotate-left", IconName::Undo2, false, "Rotate left")
                                .on_click(cx.listener(Self::rotate_left)),
                        )
                        .child(
                            square_toggle("rotate-right", IconName::Redo2, false, "Rotate right")
                                .on_click(cx.listener(Self::rotate_right)),
                        )
                        .child(
                            toggle_button("flip-horizontal", "Flip H", parameters.flip_horizontal)
                                .on_click(cx.listener(Self::toggle_flip_horizontal)),
                        )
                        .child(
                            toggle_button("flip-vertical", "Flip V", parameters.flip_vertical)
                                .on_click(cx.listener(Self::toggle_flip_vertical)),
                        ),
                ),
            )
            .child(
                deck_group("RESULT", 0.0).child(
                    div()
                        .mt(px(12.0))
                        .font_family(MONO_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(20.0))
                        .text_color(rgb(ACCENT))
                        .child(self.projected_result().map_or_else(
                            || "--".to_owned(),
                            |(width, height)| format!("{width} x {height}"),
                        )),
                ),
            )
    }

    fn crop_tools(&self, cx: &mut Context<Self>) -> Div {
        let crop = self.current_crop();
        deck_body()
            .child(
                deck_group("CROP", 340.0).child(
                    div()
                        .mt(px(10.0))
                        .flex()
                        .gap(px(6.0))
                        .child(
                            quiet_button("reset-crop", "Reset", true)
                                .on_click(cx.listener(Self::reset_crop)),
                        )
                        .child(
                            quiet_button(
                                "crop-enabled",
                                if self.crop_enabled {
                                    "Remove"
                                } else {
                                    "Enable"
                                },
                                true,
                            )
                            .on_click(cx.listener(Self::toggle_crop)),
                        ),
                ),
            )
            .child(
                deck_group("RESULT", 0.0).child(
                    div()
                        .mt(px(12.0))
                        .font_family(MONO_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(20.0))
                        .text_color(rgb(ACCENT))
                        .child(crop.map_or_else(
                            || "FULL IMAGE".to_owned(),
                            |crop| format!("{} x {}", crop.width, crop.height),
                        )),
                ),
            )
    }

    fn encode_tools(&self, cx: &mut Context<Self>) -> Div {
        let parameters = self.image_parameters();
        let formats = [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Webp,
            ImageFormat::Avif,
            ImageFormat::Jxl,
            ImageFormat::Tiff,
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::Jp2,
            ImageFormat::Qoi,
            ImageFormat::Tga,
        ];
        let mut format_rows = div().mt(px(8.0)).flex().flex_col().gap(px(5.0));
        for row_formats in formats.chunks(6) {
            let mut row = div().flex().gap(px(5.0));
            for format in row_formats {
                let format = *format;
                row = row.child(
                    segment(
                        format.extension(),
                        format.label(),
                        parameters.format == format,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_format(format, cx);
                    })),
                );
            }
            format_rows = format_rows.child(row);
        }

        deck_body()
            .child(deck_group("FORMAT", 500.0).child(format_rows))
            .child(deck_group("QUALITY", 310.0).when(
                parameters.format.has_quality_control(),
                |group| {
                    group.child(slider_row(
                        "Quality",
                        parameters.quality.to_string(),
                        &self.quality_slider,
                    ))
                },
            ))
            .child(
                deck_group("META", 210.0).child(
                    div()
                        .mt(px(8.0))
                        .flex()
                        .gap(px(5.0))
                        .child(
                            segment(
                                "metadata-strip",
                                "Remove",
                                parameters.metadata == MetadataPolicy::Strip,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_metadata(MetadataPolicy::Strip, cx);
                            })),
                        )
                        .child(
                            segment(
                                "metadata-preserve",
                                "Keep",
                                parameters.metadata == MetadataPolicy::Preserve,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_metadata(MetadataPolicy::Preserve, cx);
                            })),
                        ),
                ),
            )
    }

    fn queue_strip(&self) -> Div {
        let mut jobs = div().flex_1().min_w(px(0.0)).flex();
        for job in self.jobs.iter().take(4) {
            let name = self
                .assets
                .get(job.asset_index)
                .map_or("image", |asset| asset.name.as_str());
            jobs = jobs.child(compact_job(name, &job.snapshot));
        }
        div()
            .h(px(66.0))
            .flex_none()
            .border_t_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(STATS_BACKGROUND))
            .flex()
            .child(jobs)
    }

    fn status_bar(&self) -> Div {
        let ready = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.status == EngineStatus::Ready);
        div()
            .h(px(STATUS_HEIGHT))
            .flex_none()
            .px(px(8.0))
            .border_t_1()
            .border_color(rgb(BORDER_SOFT))
            .bg(rgb(TITLEBAR))
            .font_family(MONO_FONT)
            .text_size(px(9.0))
            .text_color(rgb(DIM))
            .flex()
            .items_center()
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .size(px(5.0))
                    .bg(rgb(if ready { SUCCESS } else { ERROR })),
            )
            .child(
                div()
                    .ml(px(6.0))
                    .text_color(rgb(if ready { SUCCESS } else { ERROR }))
                    .child(if ready { "NATIVE" } else { "OFFLINE" }),
            )
    }
}

fn title_icon_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    selected: bool,
) -> Button {
    Button::new(id)
        .icon(icon)
        .compact()
        .selected(selected)
        .tooltip(tooltip)
}

fn window_control(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    action: impl Fn(&mut Window) + 'static,
) -> Stateful<Div> {
    let close = area == WindowControlArea::Close;
    div()
        .id(id)
        .w(px(38.0))
        .h_full()
        .flex_none()
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(MUTED))
        .hover(move |style| {
            if close {
                style.bg(rgb(CLOSE_HOVER)).text_color(rgb(0xffffff))
            } else {
                style.bg(rgb(HOVER)).text_color(rgb(TEXT))
            }
        })
        .child(Icon::new(icon).size(px(12.0)))
        .on_click(move |_, window, _| action(window))
}

fn command_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    enabled: bool,
    danger: bool,
) -> Stateful<Div> {
    let color = if !enabled {
        DIM
    } else if danger {
        ERROR
    } else {
        ACCENT
    };
    div()
        .id(id)
        .h(px(29.0))
        .min_w(px(78.0))
        .px(px(10.0))
        .border_1()
        .border_color(rgb(if enabled { color } else { BORDER_SOFT }))
        .bg(rgb(APP_BACKGROUND))
        .text_color(rgb(color))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .font_family(MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(11.0))
        .when(enabled, |button| {
            button.hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        })
        .child(Icon::new(icon).size(px(13.0)))
        .child(label)
}

fn quiet_button(id: &'static str, label: &'static str, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(25.0))
        .px(px(7.0))
        .text_color(rgb(if enabled { MUTED } else { DIM }))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .text_size(px(10.0))
        .when(enabled, |button| {
            button.hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        })
        .child(label)
}

fn stat_block(label: &'static str, value: String, detail: String, color: u32) -> Div {
    div()
        .min_w(px(0.0))
        .flex_1()
        .h_full()
        .px(px(14.0))
        .border_r_1()
        .border_color(rgb(BORDER_SOFT))
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .font_family(MONO_FONT)
                .text_size(px(10.0))
                .text_color(rgb(DIM))
                .child(label),
        )
        .child(
            div()
                .mt(px(3.0))
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(MONO_FONT)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(20.0))
                .text_color(rgb(color))
                .child(value),
        )
        .child(
            div()
                .mt(px(2.0))
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(MONO_FONT)
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}

fn check_box(active: bool) -> Div {
    div()
        .size(px(14.0))
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active { ACCENT } else { APP_BACKGROUND }))
        .flex()
        .items_center()
        .justify_center()
        .when(active, |box_| {
            box_.child(
                Icon::new(IconName::Check)
                    .size(px(10.0))
                    .text_color(rgb(APP_BACKGROUND)),
            )
        })
}

fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.0))
        .min_w(px(52.0))
        .px(px(9.0))
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active {
            ACTIVE_CONTROL
        } else {
            APP_BACKGROUND
        }))
        .text_color(rgb(if active { ACCENT } else { MUTED }))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(label.into())
}

fn toggle_button(id: &'static str, label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.0))
        .px(px(8.0))
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active {
            ACTIVE_CONTROL
        } else {
            APP_BACKGROUND
        }))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(7.0))
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .text_color(rgb(if active { TEXT } else { MUTED }))
        .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(check_box(active))
        .child(label)
}

fn square_toggle(id: &'static str, icon: IconName, active: bool, tooltip: &'static str) -> Button {
    Button::new(id)
        .icon(icon)
        .compact()
        .selected(active)
        .tooltip(tooltip)
}

fn tool_tab(id: &'static str, label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h_full()
        .px(px(12.0))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(11.0))
        .text_color(rgb(if active { TEXT } else { DIM }))
        .hover(|style| style.text_color(rgb(TEXT)))
        .when(active, |tab| {
            tab.child(
                div()
                    .absolute()
                    .left(px(10.0))
                    .right(px(10.0))
                    .bottom_0()
                    .h(px(2.0))
                    .bg(rgb(ACCENT)),
            )
        })
        .child(label)
}

fn deck_body() -> Div {
    div().flex_1().min_h(px(0.0)).min_w(px(0.0)).flex()
}

fn deck_group(label: &'static str, width: f32) -> Div {
    div()
        .h_full()
        .min_w(px(0.0))
        .when(width > 0.0, |group| group.w(px(width)).flex_none())
        .when(width <= 0.0, |group| group.flex_1())
        .px(px(14.0))
        .py(px(11.0))
        .border_r_1()
        .border_color(rgb(BORDER_SOFT))
        .flex()
        .flex_col()
        .when(!label.is_empty(), |group| {
            group.child(
                div()
                    .font_family(MONO_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(10.0))
                    .text_color(rgb(DIM))
                    .child(label),
            )
        })
}

fn deck_input(
    input: &Entity<gpui_component::input::InputState>,
    label: &'static str,
    unit: &'static str,
) -> Div {
    div()
        .min_w(px(0.0))
        .flex_1()
        .child(
            div()
                .mb(px(5.0))
                .font_family(MONO_FONT)
                .text_size(px(10.0))
                .text_color(rgb(DIM))
                .child(format!("{label}  {unit}")),
        )
        .child(
            div()
                .h(px(35.0))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(INPUT_BACKGROUND))
                .flex()
                .items_center()
                .child(
                    Input::new(input)
                        .appearance(false)
                        .min_w(px(0.0))
                        .flex_1()
                        .font_family(MONO_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(14.0)),
                ),
        )
}

fn slider_row(
    label: &'static str,
    value: String,
    slider: &Entity<gpui_component::slider::SliderState>,
) -> Div {
    div()
        .mt(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(label)
                .child(div().flex_1())
                .child(
                    div()
                        .min_w(px(46.0))
                        .text_align(TextAlign::Right)
                        .text_color(rgb(TEXT))
                        .child(value),
                ),
        )
        .child(
            div()
                .mt(px(5.0))
                .h(px(19.0))
                .flex()
                .items_center()
                .child(Slider::new(slider).w_full()),
        )
}

fn crop_handle(left: f32, top: f32) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .size(px(10.0))
        .border_1()
        .border_color(rgb(APP_BACKGROUND))
        .bg(rgb(TEXT))
}

fn compact_job(name: &str, snapshot: &JobSnapshot) -> Div {
    let state = snapshot.state.unwrap_or(JobState::Queued);
    let color = state_color(state);
    let progress = if snapshot.progress_total == 0 {
        0.0
    } else {
        snapshot.progress_completed as f32 / snapshot.progress_total as f32
    };
    div()
        .min_w(px(0.0))
        .flex_1()
        .h_full()
        .px(px(10.0))
        .border_r_1()
        .border_color(rgb(BORDER_SOFT))
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().size(px(6.0)).bg(rgb(color)))
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(MONO_FONT)
                        .text_size(px(10.0))
                        .text_color(rgb(TEXT))
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(px(9.0))
                        .text_color(rgb(color))
                        .child(format!("{state:?}")),
                ),
        )
        .child(
            div()
                .mt(px(7.0))
                .h(px(2.0))
                .w_full()
                .bg(rgb(BORDER_SOFT))
                .child(div().h_full().w(relative(progress)).bg(rgb(color))),
        )
}

fn error_banner(message: String) -> Div {
    div()
        .absolute()
        .left(px(10.0))
        .right(px(10.0))
        .bottom(px(10.0))
        .min_h(px(34.0))
        .px(px(9.0))
        .border_1()
        .border_color(rgb(ERROR))
        .bg(rgb(APP_BACKGROUND))
        .flex()
        .items_center()
        .font_family(MONO_FONT)
        .text_size(px(10.0))
        .text_color(rgb(ERROR))
        .child(message)
}

fn empty_message(label: &'static str) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(12.0))
        .text_color(rgb(DIM))
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

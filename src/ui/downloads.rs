//! The downloads panel, above the status bar.

use gpui::{
    Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::StyledExt as _;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    v_flex,
};

use super::util::human_size;
use super::{DownloadStatus, Studio};
use crate::assets::Lucide;

impl Studio {
    pub fn render_downloads_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let finished = self.downloads.len() - self.running_downloads();

        v_flex()
            .id("downloads-panel")
            .absolute()
            .right(px(12.))
            .bottom(px(36.))
            .w(px(420.))
            .max_h(px(420.))
            .occlude()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Downloads"))
                    .child(
                        h_flex()
                            .gap_1()
                            .when(finished > 0, |d| {
                                d.child(
                                    Button::new("clear-downloads")
                                        .ghost()
                                        .xsmall()
                                        .label("Clear finished")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.clear_finished_downloads(cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("close-downloads")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.downloads_open = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("downloads-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_2()
                    .when(self.downloads.is_empty(), |d| {
                        d.child(
                            v_flex()
                                .py_6()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(Icon::from(Lucide::HardDriveDownload).size(px(28.)))
                                .child("No downloads"),
                        )
                    })
                    .children(self.downloads.iter().map(|d| {
                        let (done, total) = d.progress.snapshot();
                        let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                        let elapsed = d.started.elapsed().as_secs_f32().max(0.1);
                        let speed = done as f32 / elapsed;
                        let id = d.id;
                        let status: gpui::AnyElement = match &d.status {
                            DownloadStatus::Running => h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(if total > 0 {
                                    format!(
                                        "{} / {} · {}/s",
                                        human_size(done),
                                        human_size(total),
                                        human_size(speed as u64)
                                    )
                                } else {
                                    "Starting…".to_string()
                                })
                                .into_any_element(),
                            DownloadStatus::Done(name) => div()
                                .text_xs()
                                .text_color(theme.success)
                                .child(format!("Installed as {name}"))
                                .into_any_element(),
                            DownloadStatus::Failed(e) => div()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(e.clone())
                                .into_any_element(),
                            DownloadStatus::Cancelled => div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Cancelled — downloading again resumes it")
                                .into_any_element(),
                        };
                        let running = matches!(d.status, DownloadStatus::Running);
                        let installed = match &d.status {
                            DownloadStatus::Done(n) => Some(n.clone()),
                            _ => None,
                        };
                        v_flex()
                            .p_2()
                            .gap_1()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().truncate().text_sm().child(d.name.clone()))
                                    .when(running, |r| {
                                        r.child(
                                            Button::new(("cancel-download", id as usize))
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Close)
                                                .tooltip("Cancel")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.cancel_download(id, cx)
                                                })),
                                        )
                                    })
                                    .when_some(installed, |r, name| {
                                        r.child(
                                            Button::new(("load-download", id as usize))
                                                .xsmall()
                                                .primary()
                                                .label("Load")
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.downloads_open = false;
                                                    this.picker = None;
                                                    this.open_picker(window, cx);
                                                    this.picker_select(name.clone(), window, cx);
                                                })),
                                        )
                                    }),
                            )
                            .when(running, |r| {
                                r.child(
                                    Progress::new(("download-progress", id as usize))
                                        .value(fraction * 100.0)
                                        .loading(total == 0),
                                )
                            })
                            .child(status)
                    })),
            )
    }
}

//! Developer: the local OpenAI-compatible server, and the logs.

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement, StatefulInteractiveElement as _,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, StyledExt as _};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    switch::Switch,
    text::TextView,
    v_flex,
};

use super::Studio;
use super::util::section_label;
use crate::logs::Level;

const ENDPOINTS: &[(&str, &str)] = &[
    ("GET", "/v1/models"),
    ("POST", "/v1/chat/completions"),
    ("POST", "/v1/completions"),
    ("POST", "/v1/embeddings"),
    ("GET", "/health"),
];

impl Studio {
    pub fn render_developer(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let running = self.server.is_some();
        let addr = self.server.as_ref().map(|s| s.addr);
        let base = addr
            .map(|a| format!("http://{a}"))
            .unwrap_or_else(|| {
                format!(
                    "http://{}:{}",
                    self.settings.server_host, self.settings.server_port
                )
            });
        let example_model = self
            .model_state
            .loaded()
            .map(|m| m.name.clone())
            .or_else(|| self.local_models.first().map(|m| m.name.clone()))
            .unwrap_or_else(|| "qwen2.5:7b".into());
        let curl = format!(
            "```sh\ncurl {base}/v1/chat/completions \\\n  -H 'content-type: application/json' \\\n  -d '{{\"model\": \"{example_model}\", \"messages\": [{{\"role\": \"user\", \"content\": \"Say hello\"}}]}}'\n```"
        );
        let base_copy = format!("{base}/v1");

        let server_card = v_flex()
            .p_4()
            .gap_3()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        v_flex()
                            .child(div().text_base().font_semibold().child("Local Server"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("An OpenAI-compatible API over the installed models. It loads models on demand, separately from the chat."),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .text_xs()
                                    .when(running, |d| d.bg(theme.success.opacity(0.2)).text_color(theme.success))
                                    .when(!running, |d| d.bg(theme.secondary).text_color(theme.muted_foreground))
                                    .child(if running { "Running" } else if self.server_busy { "Starting…" } else { "Stopped" }),
                            )
                            .child(
                                Switch::new("server-switch")
                                    .checked(running)
                                    .disabled(self.server_busy)
                                    .on_click(cx.listener(|this, _, window, cx| this.toggle_server(window, cx))),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .w(px(200.))
                            .child(section_label("HOST", cx))
                            .child(Input::new(&self.server_host).small().disabled(running)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .w(px(120.))
                            .child(section_label("PORT", cx))
                            .child(Input::new(&self.server_port).small().disabled(running)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .child(section_label("BASE URL", cx))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .h(px(28.))
                                    .child(div().text_sm().font_family("monospace").child(base_copy.clone()))
                                    .child(
                                        Button::new("copy-base")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Copy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.copy(base_copy.clone(), window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(section_label("ENDPOINTS", cx))
                    .children(ENDPOINTS.iter().map(|(method, path)| {
                        h_flex()
                            .gap_2()
                            .text_sm()
                            .font_family("monospace")
                            .child(
                                div()
                                    .w(px(48.))
                                    .text_color(if *method == "GET" { theme.success } else { theme.info })
                                    .child(*method),
                            )
                            .child(*path)
                    })),
            )
            .child(
                div().text_xs().child(TextView::markdown("curl-example", curl).selectable(true)),
            );

        let logs = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Logs"))
                    .child(
                        Button::new("clear-logs")
                            .ghost()
                            .xsmall()
                            .label("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.logs.clear();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("logs")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.logs_scroll)
                    .p_2()
                    .font_family("monospace")
                    .text_xs()
                    .children(self.logs.iter().map(|l| {
                        let color = match l.level {
                            Level::Error => theme.danger,
                            Level::Warn => theme.warning,
                            Level::Info => theme.foreground,
                        };
                        h_flex()
                            .gap_2()
                            .items_start()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(theme.muted_foreground)
                                    .child(l.time.format("%H:%M:%S").to_string()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(72.))
                                    .text_color(theme.primary)
                                    .child(l.source),
                            )
                            .child(div().flex_1().min_w_0().text_color(color).child(l.text.clone()))
                    }))
                    .when(self.logs.is_empty(), |d| {
                        d.child(div().text_color(theme.muted_foreground).child("Nothing logged yet"))
                    }),
            );

        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .child(div().text_lg().font_semibold().child("Developer"))
            .child(server_card)
            .child(logs)
    }
}

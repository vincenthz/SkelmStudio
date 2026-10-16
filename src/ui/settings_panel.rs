//! The right panel of the chat page: the chat's settings (system prompt,
//! sampling), or the settings of a project.

use gpui::{
    AnyElement, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, StyledExt as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, Textarea},
    slider::{Slider, SliderState},
    v_flex,
};

use super::util::{human_size, info_row, section_label};
use super::{RightPanel, Studio};
use crate::assets::Lucide;
use crate::store::Id;

impl Studio {
    pub fn render_settings_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let content: AnyElement = match self.right_panel.clone() {
            RightPanel::Project(id) if self.project(&id).is_some() => {
                self.render_project_settings(&id, window, cx).into_any_element()
            }
            _ => self.render_chat_settings(window, cx).into_any_element(),
        };
        v_flex()
            .h_full()
            .w(px(320.))
            .flex_shrink_0()
            .bg(theme.sidebar)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                v_flex()
                    .id("settings-panel")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
    }

    fn slider_row(
        &self,
        label: &'static str,
        hint: &'static str,
        slider: &Entity<SliderState>,
        value: SharedString,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .id(label)
                            .text_sm()
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(hint).build(window, cx)
                            })
                            .child(label),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .rounded(px(4.))
                            .bg(theme.secondary)
                            .text_xs()
                            .child(value),
                    ),
            )
            .child(Slider::new(slider))
    }

    fn render_chat_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let Some(chat) = self.active() else {
            return div().into_any_element();
        };
        let s = chat.sampling.clone();
        let project = self.project_of(chat).map(|p| (p.id.clone(), p.name.clone()));
        let loaded = self.model_state.loaded().cloned();

        v_flex()
            .p_3()
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("Chat Settings"))
                    .child(
                        Button::new("close-settings")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.show_right_panel = false;
                                this.save_settings();
                                cx.notify();
                            })),
                    ),
            )
            .when_some(project, |d, (pid, name)| {
                d.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .text_sm()
                        .child(Icon::new(IconName::Folder).size_4().text_color(theme.primary))
                        .child(div().flex_1().truncate().child(name))
                        .child(
                            Button::new("open-project")
                                .xsmall()
                                .ghost()
                                .label("Project settings")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_project_settings(pid.clone(), window, cx)
                                })),
                        ),
                )
            })
            .child(
                v_flex()
                    .gap_1p5()
                    .child(section_label("SYSTEM PROMPT", cx))
                    .child(Textarea::new(&self.system_prompt)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(section_label("SETTINGS", cx))
                    .child(self.slider_row(
                        "Temperature",
                        "Randomness of the output: 0 always picks the most likely token",
                        &self.sliders.temperature,
                        format!("{:.2}", s.temperature).into(),
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child("Limit Response Length"))
                            .child(Input::new(&self.max_tokens).small().suffix(
                                div().text_xs().text_color(theme.muted_foreground).child("tokens"),
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(section_label("SAMPLING", cx))
                    .child(self.slider_row(
                        "Top K Sampling",
                        "Sample among the K most likely tokens (0 disables)",
                        &self.sliders.top_k,
                        if s.top_k == 0 {
                            "off".into()
                        } else {
                            s.top_k.to_string().into()
                        },
                        cx,
                    ))
                    .child(self.slider_row(
                        "Top P Sampling",
                        "Sample among the most likely tokens adding up to P (1 disables)",
                        &self.sliders.top_p,
                        format!("{:.2}", s.top_p).into(),
                        cx,
                    ))
                    .child(self.slider_row(
                        "Min P Sampling",
                        "Drop tokens less likely than P times the most likely one (0 disables)",
                        &self.sliders.min_p,
                        format!("{:.2}", s.min_p).into(),
                        cx,
                    ))
                    .child(self.slider_row(
                        "Repeat Penalty",
                        "Penalize tokens repeated among the last 64 (1 disables)",
                        &self.sliders.repeat_penalty,
                        format!("{:.2}", s.repeat_penalty).into(),
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child("Seed"))
                            .child(Input::new(&self.seed).small()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("reset-sampling")
                            .small()
                            .ghost()
                            .icon(Lucide::RotateCcw)
                            .label("Reset")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reset_sampling(window, cx)
                            })),
                    )
                    .child(
                        Button::new("default-sampling")
                            .small()
                            .outline()
                            .label("Use for new chats")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_sampling_as_default(window, cx)
                            })),
                    ),
            )
            .when_some(loaded, |d, m| {
                d.child(
                    v_flex()
                        .gap_1p5()
                        .child(section_label("LOADED MODEL", cx))
                        .child(info_row("Model", m.name.clone(), cx))
                        .when_some(m.architecture.clone(), |d, a| {
                            d.child(info_row("Architecture", a, cx))
                        })
                        .child(info_row("Description", m.description.clone(), cx))
                        .child(info_row(
                            "Context",
                            format!("{} (trained {})", m.context_length, m.train_context_length),
                            cx,
                        ))
                        .child(info_row(
                            "GPU layers",
                            if m.config.gpu_layers < 0 {
                                "all".to_string()
                            } else {
                                m.config.gpu_layers.to_string()
                            },
                            cx,
                        ))
                        .child(info_row(
                            "Speculative",
                            if m.speculative {
                                "MTP"
                            } else if m.has_mtp {
                                "off (MTP available)"
                            } else {
                                "off"
                            },
                            cx,
                        ))
                        .child(info_row(
                            "Load time",
                            format!("{:.1}s", m.load_ms as f32 / 1000.0),
                            cx,
                        )),
                )
            })
            .into_any_element()
    }

    fn render_project_settings(&mut self, id: &Id, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let Some(project) = self.project(id).cloned() else {
            return div().into_any_element();
        };
        let chats = self
            .chats
            .iter()
            .filter(|c| c.project.as_deref() == Some(id))
            .count();
        let loaded = self.model_state.loaded().map(|m| m.name.clone());
        let context_chars: usize = project.files.iter().map(|f| f.content.len()).sum();
        let (id1, id2, id3, id4) = (id.clone(), id.clone(), id.clone(), id.clone());

        v_flex()
            .p_3()
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Icon::new(IconName::FolderOpen).size_4().text_color(theme.primary))
                            .child(div().text_sm().font_semibold().child("Project")),
                    )
                    .child(
                        Button::new("close-project")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Back to chat settings")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_projects();
                                this.right_panel = RightPanel::Settings;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(section_label("NAME", cx))
                    .child(Input::new(&self.project_name).small()),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(section_label("INSTRUCTIONS", cx))
                    .child(Textarea::new(&self.project_instructions))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Prepended to the system prompt of every chat in the project."),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(section_label("REFERENCE FILES", cx))
                            .child(
                                Button::new("add-files")
                                    .xsmall()
                                    .ghost()
                                    .icon(Lucide::FilePlus)
                                    .label("Add")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.add_project_files(id1.clone(), window, cx)
                                    })),
                            ),
                    )
                    .when(project.files.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Text files given to the model as context in every chat."),
                        )
                    })
                    .children(project.files.iter().enumerate().map(|(ix, f)| {
                        let pid = id.clone();
                        h_flex()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .child(Icon::new(IconName::FileText).size_4().text_color(theme.muted_foreground))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_sm().truncate().child(f.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(human_size(f.content.len() as u64)),
                                    ),
                            )
                            .child(
                                Button::new(("remove-file", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(p) = this.projects.iter_mut().find(|p| p.id == pid) {
                                            if ix < p.files.len() {
                                                p.files.remove(ix);
                                            }
                                        }
                                        this.save_projects();
                                        cx.notify();
                                    })),
                            )
                    }))
                    .when(context_chars > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "≈ {} tokens of context",
                                    context_chars / 4
                                )),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(section_label("DEFAULT MODEL", cx))
                    .child(
                        div()
                            .text_sm()
                            .child(project.default_model.clone().unwrap_or_else(|| "None".into())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Loaded when a chat of the project is opened with no model loaded."),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("use-loaded")
                                    .xsmall()
                                    .outline()
                                    .label("Use loaded model")
                                    .disabled(loaded.is_none())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let loaded = this.model_state.loaded().map(|m| m.name.clone());
                                        if let Some(p) = this.projects.iter_mut().find(|p| p.id == id2) {
                                            p.default_model = loaded;
                                        }
                                        this.save_projects();
                                        cx.notify();
                                    })),
                            )
                            .when(project.default_model.is_some(), |d| {
                                d.child(
                                    Button::new("clear-default")
                                        .xsmall()
                                        .ghost()
                                        .label("Clear")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(p) = this.projects.iter_mut().find(|p| p.id == id3) {
                                                p.default_model = None;
                                            }
                                            this.save_projects();
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("project-new-chat")
                            .small()
                            .primary()
                            .icon(Lucide::MessageSquarePlus)
                            .label("New chat")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.save_projects();
                                this.new_chat(Some(id4.clone()), window, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{chats} chat{}", if chats == 1 { "" } else { "s" })),
                    ),
            )
            .into_any_element()
    }
}

//! "My Models": the installed models, their metadata, and loading or
//! deleting them.

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, StyledExt as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    text::TextView,
    v_flex,
};

use super::picker::matches_filter;
use super::util::{ago, chip, human_size, info_row, section_label};
use super::{Page, Studio};
use crate::assets::Lucide;
use crate::catalog::{LocalModel, ModelSource};

impl Studio {
    pub fn render_models(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let filter = self.models_filter.read(cx).value().to_string();
        let models: Vec<LocalModel> = self
            .local_models
            .iter()
            .filter(|m| matches_filter(m, &filter))
            .cloned()
            .collect();
        let total: u64 = self.local_models.iter().map(|m| m.size).sum();
        let loaded = self.model_state.loaded().map(|m| m.name.clone());

        let header_cell = |text: &'static str| {
            div()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(text)
        };

        let table = v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1().child(header_cell("MODEL")))
                    .child(div().w(px(140.)).child(header_cell("PUBLISHER")))
                    .child(div().w(px(110.)).child(header_cell("SOURCE")))
                    .child(div().w(px(90.)).text_right().child(header_cell("SIZE")))
                    .child(div().w(px(90.)).text_right().child(header_cell("MODIFIED"))),
            )
            .child(
                v_flex()
                    .id("models-table")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(models.iter().enumerate().map(|(ix, m)| {
                        let selected = self.selected_model.as_deref() == Some(&m.name);
                        let is_loaded = loaded.as_deref() == Some(&m.name);
                        let name = m.name.clone();
                        h_flex()
                            .id(("model-row", ix))
                            .px_4()
                            .py_2()
                            .gap_4()
                            .cursor_pointer()
                            .border_b_1()
                            .border_color(theme.border.opacity(0.5))
                            .when(selected, |d| d.bg(theme.accent))
                            .when(!selected, |d| d.hover(|s| s.bg(theme.accent.opacity(0.4))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_model = Some(name.clone());
                                this.inspect_model(&name.clone(), cx);
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_2()
                                    .child(Icon::from(Lucide::Box).size_4().text_color(theme.muted_foreground))
                                    .child(div().text_sm().font_medium().truncate().child(m.family.clone()))
                                    .child(chip(m.variant.clone(), cx))
                                    .when(is_loaded, |d| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.success)
                                                .child("● Loaded"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(140.))
                                    .truncate()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(m.publisher.clone()),
                            )
                            .child(div().w(px(110.)).text_sm().child(m.source.label()))
                            .child(div().w(px(90.)).text_right().text_sm().child(human_size(m.size)))
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_right()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(ago(m.modified)),
                            )
                    }))
                    .when(models.is_empty(), |d| {
                        d.child(
                            v_flex()
                                .py_16()
                                .items_center()
                                .gap_3()
                                .text_color(theme.muted_foreground)
                                .child(Icon::from(Lucide::Package).size(px(32.)))
                                .child(if self.local_models.is_empty() {
                                    "No models installed"
                                } else {
                                    "No model matches the filter"
                                })
                                .child(
                                    Button::new("models-discover")
                                        .primary()
                                        .small()
                                        .icon(IconName::Search)
                                        .label("Discover models")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_page(Page::Discover, window, cx)
                                        })),
                                ),
                        )
                    }),
            );

        let details = self
            .selected_model
            .clone()
            .and_then(|name| self.local_models.iter().find(|m| m.name == name).cloned())
            .map(|m| self.render_model_details(&m, window, cx));

        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .px_4()
                            .pt_4()
                            .pb_3()
                            .gap_3()
                            .justify_between()
                            .child(
                                v_flex()
                                    .child(div().text_lg().font_semibold().child("My Models"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(format!(
                                                "{} models · {} on disk · ~/.ollama and the Hugging Face cache",
                                                self.local_models.len(),
                                                human_size(total)
                                            )),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div().w(px(240.)).child(
                                            Input::new(&self.models_filter)
                                                .small()
                                                .cleanable(true)
                                                .prefix(Icon::new(IconName::Search).size_3p5()),
                                        ),
                                    )
                                    .child(
                                        Button::new("models-refresh")
                                            .small()
                                            .ghost()
                                            .icon(IconName::RefreshCw)
                                            .tooltip("Refresh")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_models(cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(table),
            )
            .children(details)
    }

    fn render_model_details(&mut self, m: &LocalModel, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let info = self.model_info.get(&m.name).cloned();
        let is_loaded = self
            .model_state
            .loaded()
            .is_some_and(|l| l.name == m.name);
        let weak = cx.entity().downgrade();
        let (name1, name2) = (m.name.clone(), m.name.clone());
        let path = m.path.clone();

        v_flex()
            .w(px(380.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                v_flex()
                    .id("model-details")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child(m.family.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(m.name.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("details-load")
                                    .small()
                                    .primary()
                                    .icon(Lucide::Box)
                                    .label(if is_loaded { "Loaded" } else { "Load" })
                                    .disabled(is_loaded)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.picker = None;
                                        this.open_picker(window, cx);
                                        this.picker_select(name1.clone(), window, cx);
                                    })),
                            )
                            .when_some(path.clone(), |d, path| {
                                d.child(
                                    Button::new("details-reveal")
                                        .small()
                                        .outline()
                                        .icon(IconName::FolderOpen)
                                        .label("Show in Finder")
                                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                                )
                            })
                            .child(
                                Button::new("details-delete")
                                    .small()
                                    .ghost()
                                    .icon(Lucide::Trash)
                                    .tooltip("Delete model")
                                    .on_click(move |_, window, cx| {
                                        confirm_delete_model(weak.clone(), name2.clone(), window, cx)
                                    }),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(section_label("FILE", cx))
                            .child(info_row("Source", m.source.label(), cx))
                            .child(info_row(
                                if m.source == ModelSource::HuggingFace {
                                    "Quantization"
                                } else {
                                    "Variant"
                                },
                                m.variant.clone(),
                                cx,
                            ))
                            .child(info_row("Size", human_size(m.size), cx))
                            .child(info_row("Modified", ago(m.modified), cx))
                            .when_some(path, |d, p| {
                                d.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(p.display().to_string()),
                                )
                            }),
                    )
                    .child(match info {
                        None => h_flex()
                            .gap_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(gpui_component::spinner::Spinner::new().small())
                            .child("Reading GGUF metadata…")
                            .into_any_element(),
                        Some(Err(e)) => div()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(e)
                            .into_any_element(),
                        Some(Ok(info)) => {
                            let template = info.chat_template.clone();
                            v_flex()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .gap_1p5()
                                        .child(section_label("MODEL", cx))
                                        .when_some(info.name.clone(), |d, v| d.child(info_row("Name", v, cx)))
                                        .when_some(info.architecture.clone(), |d, v| {
                                            d.child(info_row("Architecture", v, cx))
                                        })
                                        .when_some(info.size_label.clone(), |d, v| {
                                            d.child(info_row("Parameters", v, cx))
                                        })
                                        .when(!info.description.is_empty(), |d| {
                                            d.child(info_row("Type", info.description.clone(), cx))
                                        })
                                        .when_some(info.context_length, |d, v| {
                                            d.child(info_row("Context length", v.to_string(), cx))
                                        })
                                        .child(info_row("Vocabulary", info.vocab_size.to_string(), cx))
                                        .when_some(info.license.clone(), |d, v| d.child(info_row("License", v, cx))),
                                )
                                .child(
                                    v_flex()
                                        .gap_1p5()
                                        .child(section_label("CAPABILITIES", cx))
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .flex_wrap()
                                                .child(capability("Chat template", template.is_some(), cx))
                                                .child(capability("Tool use", info.supports_tools(), cx))
                                                .child(capability("Reasoning", info.supports_reasoning(), cx)),
                                        ),
                                )
                                .when_some(template, |d, t| {
                                    d.child(
                                        v_flex()
                                            .gap_1p5()
                                            .child(section_label("CHAT TEMPLATE", cx))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .child(TextView::markdown(
                                                        SharedString::from(format!("template-{}", m.name)),
                                                        format!("```jinja\n{t}\n```"),
                                                    )
                                                    .selectable(true)),
                                            ),
                                    )
                                })
                                .into_any_element()
                        }
                    }),
            )
    }
}

fn capability(label: &'static str, yes: bool, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    h_flex()
        .gap_1()
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .when(!yes, |d| d.text_color(theme.muted_foreground).line_through())
        .child(if yes {
            Icon::new(IconName::Check).size_3()
        } else {
            Icon::new(IconName::Minus).size_3()
        })
        .child(label)
}

fn confirm_delete_model(this: WeakEntity<Studio>, name: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, _| {
        let (this, name2) = (this.clone(), name.clone());
        dialog
            .title(format!("Delete {name}?"))
            .w(px(460.))
            .child(
                div()
                    .text_sm()
                    .child("The model files are removed from disk. You can download it again from Discover."),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("del-model-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("del-model-ok")
                            .danger()
                            .label("Delete")
                            .on_click(move |_, window, cx| {
                                let name = name2.clone();
                                let _ = this.update(cx, |this, cx| this.delete_model(name, window, cx));
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

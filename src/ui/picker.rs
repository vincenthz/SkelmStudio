//! The model loader (⌘L): pick an installed model and how to load it.

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, StyledExt as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    slider::{Slider, SliderScale, SliderState},
    switch::Switch,
    v_flex,
};

use super::util::{chip, human_size};
use super::{ModelState, Page, Studio};
use crate::assets::Lucide;
use crate::catalog::LocalModel;
use crate::store::{FlashAttention, LoadConfig};

const MAX_CONTEXT: f32 = 262_144.;
const MAX_GPU_LAYERS: f32 = 128.;

pub struct Picker {
    pub selected: Option<String>,
    pub config: LoadConfig,
    pub context: Entity<SliderState>,
    pub gpu_layers: Entity<SliderState>,
}

/// Round a context length to a multiple of 256
fn round_context(v: f32) -> u32 {
    ((v / 256.).round() as u32).max(2) * 256
}

pub fn matches_filter(model: &LocalModel, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || filter
            .split_whitespace()
            .all(|w| model.name.to_lowercase().contains(w))
}

impl Studio {
    pub fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.close_picker(cx);
            return;
        }
        self.refresh_models(cx);
        let selected = self
            .model_state
            .loaded()
            .map(|m| m.name.clone())
            .or_else(|| self.settings.last_model.clone())
            .filter(|n| self.local_models.iter().any(|m| &m.name == n))
            .or_else(|| self.local_models.first().map(|m| m.name.clone()));
        let config = selected
            .as_deref()
            .map(|n| self.load_config_for(n))
            .unwrap_or_else(|| self.settings.load.clone());
        let context = cx.new(|_| {
            SliderState::new()
                .max(MAX_CONTEXT)
                .min(512.)
                .scale(SliderScale::Logarithmic)
                .default_value(config.context_length.max(512) as f32)
        });
        let gpu_layers = cx.new(|_| {
            SliderState::new()
                .max(MAX_GPU_LAYERS)
                .min(0.)
                .step(1.)
                .default_value(if config.gpu_layers < 0 {
                    MAX_GPU_LAYERS
                } else {
                    config.gpu_layers as f32
                })
        });
        if let Some(name) = &selected {
            self.inspect_model(&name.clone(), cx);
        }
        self.picker = Some(Picker {
            selected,
            config,
            context,
            gpu_layers,
        });
        self.picker_search
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.picker_search.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }

    pub fn picker_select(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.load_config_for(&name);
        self.inspect_model(&name, cx);
        if let Some(p) = &mut self.picker {
            p.context.update(cx, |s, cx| {
                s.set_value(config.context_length.max(512) as f32, window, cx)
            });
            p.gpu_layers.update(cx, |s, cx| {
                s.set_value(
                    if config.gpu_layers < 0 {
                        MAX_GPU_LAYERS
                    } else {
                        config.gpu_layers as f32
                    },
                    window,
                    cx,
                )
            });
            p.selected = Some(name);
            p.config = config;
        }
        cx.notify();
    }

    /// The load settings currently shown, with the sliders' values
    fn picker_config(&self, cx: &Context<Self>) -> Option<(String, LoadConfig)> {
        let p = self.picker.as_ref()?;
        let name = p.selected.clone()?;
        let mut config = p.config.clone();
        config.context_length = round_context(p.context.read(cx).value().start());
        if config.gpu_layers >= 0 {
            config.gpu_layers = p.gpu_layers.read(cx).value().start().round() as i32;
        }
        Some((name, config))
    }

    pub fn picker_load(&mut self, cx: &mut Context<Self>) {
        if let Some((name, config)) = self.picker_config(cx) {
            self.picker = None;
            self.load_model(name, config, cx);
        }
    }

    pub fn render_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let filter = self.picker_search.read(cx).value().to_string();
        let models: Vec<LocalModel> = self
            .local_models
            .iter()
            .filter(|m| matches_filter(m, &filter))
            .cloned()
            .collect();
        let selected = self.picker.as_ref().and_then(|p| p.selected.clone());
        let loaded = self.model_state.loaded().map(|m| m.name.clone());

        let list = v_flex()
            .id("picker-list")
            .max_h(px(300.))
            .overflow_y_scroll()
            .p_1()
            .gap_0p5()
            .children(models.iter().enumerate().map(|(ix, m)| {
                let is_selected = selected.as_deref() == Some(&m.name);
                let is_loaded = loaded.as_deref() == Some(&m.name);
                let name = m.name.clone();
                h_flex()
                    .id(("picker-model", ix))
                    .px_2()
                    .py_1p5()
                    .gap_3()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .when(is_selected, |d| d.bg(theme.accent))
                    .when(!is_selected, |d| d.hover(|s| s.bg(theme.accent.opacity(0.5))))
                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                        this.picker_select(name.clone(), window, cx);
                        if ev.click_count() >= 2 {
                            this.picker_load(cx);
                        }
                    }))
                    .child(
                        Icon::from(Lucide::Box)
                            .size_4()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_sm().font_medium().truncate().child(m.family.clone()))
                                    .child(chip(m.variant.clone(), cx))
                                    .when(is_loaded, |d| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.success)
                                                .child("Loaded"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(m.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(m.source.label()),
                    )
                    .child(
                        div()
                            .w(px(72.))
                            .text_right()
                            .text_xs()
                            .child(human_size(m.size)),
                    )
            }))
            .when(models.is_empty(), |d| {
                d.child(
                    v_flex()
                        .py_8()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if self.local_models.is_empty() {
                            "No models installed yet"
                        } else {
                            "No model matches the filter"
                        })
                        .child(
                            Button::new("picker-discover")
                                .small()
                                .primary()
                                .icon(IconName::Search)
                                .label("Discover models")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_picker(cx);
                                    this.set_page(Page::Discover, window, cx);
                                })),
                        ),
                )
            });

        let config_panel = self
            .picker
            .as_ref()
            .filter(|p| p.selected.is_some())
            .map(|p| self.render_load_config(p, window, cx));

        let loading = matches!(self.model_state, ModelState::Loading { .. });

        div()
            .id("picker-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_picker(cx)),
            )
            .child(
                h_flex().w_full().justify_center().pt(px(52.)).child(
                    v_flex()
                        .id("picker")
                        .key_context("ModelPicker")
                        .w(px(680.))
                        .max_w_full()
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .border_1()
                        .border_color(theme.border)
                        .rounded(theme.radius_lg)
                        .shadow_lg()
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            h_flex()
                                .p_2()
                                .gap_2()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    Input::new(&self.picker_search)
                                        .prefix(Icon::new(IconName::Search).size_4())
                                        .appearance(false),
                                ),
                        )
                        .child(list)
                        .children(config_panel)
                        .child(
                            h_flex()
                                .p_2()
                                .gap_2()
                                .justify_between()
                                .border_t_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(format!(
                                            "{} models · ⏎ load · esc close",
                                            self.local_models.len()
                                        )),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("picker-cancel")
                                                .small()
                                                .ghost()
                                                .label("Cancel")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.close_picker(cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("picker-load")
                                                .small()
                                                .primary()
                                                .label("Load Model")
                                                .disabled(selected.is_none() || loading)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.picker_load(cx)
                                                })),
                                        ),
                                ),
                        ),
                ),
            )
    }

    fn render_load_config(
        &self,
        picker: &Picker,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let name = picker.selected.clone().unwrap_or_default();
        let info = self.model_info.get(&name);
        let train_ctx = info
            .and_then(|i| i.as_ref().ok())
            .and_then(|i| i.context_length);
        let context = round_context(picker.context.read(cx).value().start());
        let all_layers = picker.config.gpu_layers < 0;
        let layers = picker.gpu_layers.read(cx).value().start().round() as i32;
        let fa = picker.config.flash_attention;

        let row = |label: &'static str, value: SharedString| {
            h_flex()
                .justify_between()
                .text_sm()
                .child(label)
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .text_xs()
                        .child(value),
                )
        };

        let fa_button = |id: &'static str, label: &'static str, value: FlashAttention, cx: &mut Context<Self>| {
            Button::new(id)
                .xsmall()
                .label(label)
                .when(fa == value, |b| b.primary())
                .when(fa != value, |b| b.ghost())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = &mut this.picker {
                        p.config.flash_attention = value;
                    }
                    cx.notify();
                }))
        };

        let info_line: SharedString = match info {
            None => "Reading model metadata…".into(),
            Some(Err(e)) => format!("Cannot read the metadata: {e}").into(),
            Some(Ok(i)) => {
                let mut parts = Vec::new();
                if let Some(a) = &i.architecture {
                    parts.push(a.clone());
                }
                if let Some(s) = &i.size_label {
                    parts.push(format!("{s} params"));
                }
                if let Some(c) = i.context_length {
                    parts.push(format!("trained on {c} ctx"));
                }
                if i.supports_tools() {
                    parts.push("tools".into());
                }
                if i.supports_reasoning() {
                    parts.push("reasoning".into());
                }
                parts.join(" · ").into()
            }
        };

        v_flex()
            .p_3()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.secondary.opacity(0.3))
            .child(
                v_flex()
                    .child(div().text_sm().font_semibold().child(format!("Load settings · {name}")))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(info_line),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(row(
                        "Context Length",
                        match train_ctx {
                            Some(max) if context as u64 > max => {
                                format!("{context} tokens (model max {max})").into()
                            }
                            _ => format!("{context} tokens").into(),
                        },
                    ))
                    .child(Slider::new(&picker.context))
                    .child(
                        h_flex().gap_1().children(
                            [2048u32, 4096, 8192, 16384, 32768, 65536, 131072]
                                .into_iter()
                                .filter(|v| train_ctx.is_none_or(|m| *v as u64 <= m.max(2048)))
                                .map(|v| {
                                    Button::new(("ctx-preset", v as usize))
                                        .xsmall()
                                        .ghost()
                                        .label(format!("{}K", v / 1024))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if let Some(p) = &this.picker {
                                                p.context.update(cx, |s, cx| {
                                                    s.set_value(v as f32, window, cx)
                                                });
                                            }
                                            cx.notify();
                                        }))
                                }),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_sm().child("GPU Offload"))
                            .child(
                                Switch::new("gpu-all")
                                    .small()
                                    .checked(all_layers)
                                    .label("All layers")
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        if let Some(p) = &mut this.picker {
                                            p.config.gpu_layers = if *checked { -1 } else { 0 };
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(!all_layers, |d| {
                        d.child(
                            h_flex()
                                .gap_3()
                                .child(div().flex_1().child(Slider::new(&picker.gpu_layers)))
                                .child(
                                    div()
                                        .w(px(64.))
                                        .text_right()
                                        .text_xs()
                                        .child(format!("{layers} layers")),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().child("Flash Attention"))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(fa_button("fa-auto", "Auto", FlashAttention::Auto, cx))
                            .child(fa_button("fa-on", "On", FlashAttention::On, cx))
                            .child(fa_button("fa-off", "Off", FlashAttention::Off, cx)),
                    ),
            )
            .child(
                h_flex()
                    .gap_6()
                    .child(
                        Switch::new("mmap")
                            .small()
                            .checked(picker.config.mmap)
                            .label("Memory-map weights")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(p) = &mut this.picker {
                                    p.config.mmap = *checked;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("mtp")
                            .small()
                            .checked(picker.config.mtp)
                            .label("MTP speculative decoding")
                            .tooltip("Faster generation with the model's multi-token prediction layers (Qwen3.5, GLM-4.5, DeepSeek…)")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(p) = &mut this.picker {
                                    p.config.mtp = *checked;
                                }
                                cx.notify();
                            })),
                    ),
            )
    }
}

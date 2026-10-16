//! The window frame: title bar with the model loader, the navigation rail,
//! the status bar, and the overlays drawn above the pages.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::StyledExt as _;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    spinner::Spinner,
    v_flex,
};

use super::{
    ClosePicker, EjectModel, ModelState, NewChat, OpenModelPicker, Page, StopGeneration, Studio,
    ToggleChatList, ToggleSettingsPanel,
};
use crate::assets::Lucide;

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let page: AnyElement = match self.page {
            Page::Chat => self.render_chat_page(window, cx).into_any_element(),
            Page::Developer => self.render_developer(window, cx).into_any_element(),
            Page::Models => self.render_models(window, cx).into_any_element(),
            Page::Discover => self.render_discover(window, cx).into_any_element(),
        };

        v_flex()
            .id("studio")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(|this, _: &OpenModelPicker, window, cx| {
                this.open_picker(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ClosePicker, _, cx| this.close_picker(cx)))
            .on_action(cx.listener(|this, _: &NewChat, window, cx| {
                let project = this.active().and_then(|c| c.project.clone());
                this.new_chat(project, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleChatList, _, cx| {
                this.settings.show_chat_list = !this.settings.show_chat_list;
                this.save_settings();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSettingsPanel, _, cx| {
                this.settings.show_right_panel = !this.settings.show_right_panel;
                this.save_settings();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &EjectModel, _, cx| this.eject_model(cx)))
            .on_action(cx.listener(|this, _: &StopGeneration, _, cx| this.stop_generation(cx)))
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_rail(window, cx))
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
            .child(self.render_status_bar(cx))
            .when(self.downloads_open, |d| {
                d.child(self.render_downloads_panel(window, cx))
            })
            .when(self.picker.is_some(), |d| {
                d.child(self.render_picker(window, cx))
            })
    }
}

impl Studio {
    fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (label, sub, icon_el): (SharedString, Option<SharedString>, AnyElement) =
            match &self.model_state {
                ModelState::Unloaded => (
                    "Select a model to load".into(),
                    None,
                    Icon::from(Lucide::Box)
                        .size_4()
                        .text_color(theme.muted_foreground)
                        .into_any_element(),
                ),
                ModelState::Loading { name, since } => (
                    format!("Loading {name}…").into(),
                    Some(format!("{:.0}s", since.elapsed().as_secs_f32()).into()),
                    Spinner::new().small().into_any_element(),
                ),
                ModelState::Loaded(m) => (
                    m.name.clone().into(),
                    Some(format!("{} ctx", m.context_length).into()),
                    div()
                        .size_2()
                        .rounded_full()
                        .bg(theme.success)
                        .into_any_element(),
                ),
            };
        let loaded = self.model_state.loaded().is_some();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .font_semibold()
                        .child(Icon::from(Lucide::Sparkles).size_4().text_color(theme.primary))
                        .child("Skelm Studio"),
                )
                .child(
                    h_flex()
                        .id("model-loader")
                        .w(px(520.))
                        .h(px(30.))
                        .px_3()
                        .gap_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.secondary)
                        .hover(|s| s.bg(theme.secondary_hover))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx)))
                        .child(icon_el)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .when(!loaded, |d| d.text_color(theme.muted_foreground))
                                .child(label),
                        )
                        .when_some(sub, |d, sub| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(sub),
                            )
                        })
                        .when(loaded, |d| {
                            d.child(
                                Button::new("eject")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::Power)
                                    .tooltip("Eject model (⌘E)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.eject_model(cx)
                                    })),
                            )
                        })
                        .when(!loaded, |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("⌘L"),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("theme")
                                .ghost()
                                .small()
                                .icon(if self.settings.dark_mode {
                                    IconName::Sun
                                } else {
                                    IconName::Moon
                                })
                                .tooltip("Toggle theme")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_theme(window, cx)
                                })),
                        )
                        .when(self.page == Page::Chat, |d| {
                            d.child(
                                Button::new("toggle-settings")
                                    .ghost()
                                    .small()
                                    .icon(if self.settings.show_right_panel {
                                        IconName::PanelRightClose
                                    } else {
                                        IconName::PanelRightOpen
                                    })
                                    .tooltip("Toggle settings (⌘.)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.show_right_panel =
                                            !this.settings.show_right_panel;
                                        this.save_settings();
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        )
    }

    fn render_rail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let item = |id: &'static str, icon: Icon, tip: &'static str, page: Page, cx: &mut Context<Self>| {
            let active = self.page == page;
            let theme = cx.theme();
            div()
                .id(id)
                .size(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius_lg)
                .cursor_pointer()
                .when(active, |d| {
                    d.bg(theme.sidebar_accent)
                        .text_color(theme.sidebar_accent_foreground)
                })
                .when(!active, |d| {
                    d.text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.sidebar_accent.opacity(0.5)))
                })
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tip).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| this.set_page(page, window, cx)))
                .child(icon.size_5())
        };

        v_flex()
            .h_full()
            .w(px(56.))
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .py_2()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(item("rail-chat", Lucide::MessageSquare.into(), "Chat", Page::Chat, cx))
            .child(item(
                "rail-dev",
                Lucide::Terminal.into(),
                "Developer",
                Page::Developer,
                cx,
            ))
            .child(item(
                "rail-models",
                IconName::FolderOpen.into(),
                "My Models",
                Page::Models,
                cx,
            ))
            .child(item(
                "rail-discover",
                IconName::Search.into(),
                "Discover",
                Page::Discover,
                cx,
            ))
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let running = self.running_downloads();
        let model: SharedString = match &self.model_state {
            ModelState::Unloaded => "No model loaded".into(),
            ModelState::Loading { name, .. } => format!("Loading {name}…").into(),
            ModelState::Loaded(m) => {
                let mut s = format!("{} · {} ctx", m.name, m.context_length);
                if m.speculative {
                    s.push_str(" · MTP");
                }
                s.into()
            }
        };
        let generating: Option<SharedString> = self.generation.as_ref().map(|g| {
            match g.prompt_done {
                None => "Processing prompt…".to_string(),
                Some(t) => {
                    let secs = t.elapsed().as_secs_f32();
                    if secs > 0.2 && g.chunks > 1 {
                        format!("Generating · {:.1} tok/s", g.chunks as f32 / secs)
                    } else {
                        "Generating…".to_string()
                    }
                }
            }
            .into()
        });

        h_flex()
            .h(px(28.))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_4()
            .justify_between()
            .text_xs()
            .text_color(theme.muted_foreground)
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .gap_4()
                    .child(format!("skelm-studio {}", env!("CARGO_PKG_VERSION")))
                    .child(super::util::icon_label(Lucide::Box, model, true, cx))
                    .when_some(generating, |d, g| {
                        d.child(
                            h_flex()
                                .gap_1()
                                .text_color(theme.primary)
                                .child(Spinner::new().xsmall())
                                .child(g),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when_some(self.server.as_ref().map(|s| s.addr), |d, addr| {
                        d.child(
                            h_flex()
                                .gap_1()
                                .child(div().size_1p5().rounded_full().bg(theme.success))
                                .child(format!("Server on {addr}")),
                        )
                    })
                    .child(
                        Button::new("downloads")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Download)
                            .label(if running > 0 {
                                format!("Downloads ({running})")
                            } else {
                                "Downloads".to_string()
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.downloads_open = !this.downloads_open;
                                cx.notify();
                            })),
                    ),
            )
    }
}

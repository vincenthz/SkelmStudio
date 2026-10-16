//! The chat list: search, pinned chats, projects and their chats, and the
//! other chats grouped by date.

use chrono::Local;
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::StyledExt as _;
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

use super::Studio;
use super::util::section_label;
use crate::assets::Lucide;
use crate::store::{Chat, Id};

fn matches(chat: &Chat, filter: &str) -> bool {
    filter.is_empty()
        || chat.title.to_lowercase().contains(filter)
        || chat
            .messages
            .iter()
            .any(|m| m.content.to_lowercase().contains(filter))
}

impl Studio {
    pub fn render_chat_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let filter = self.chat_filter.read(cx).value().trim().to_lowercase();
        let filtering = !filter.is_empty();

        let mut sections: Vec<AnyElement> = Vec::new();

        let pinned: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|c| c.pinned && matches(c, &filter))
            .collect();
        if !pinned.is_empty() {
            sections.push(section_header("Pinned", cx).into_any_element());
            for chat in pinned {
                sections.push(self.render_chat_row(chat, 0, window, cx).into_any_element());
            }
        }

        // projects
        sections.push(
            h_flex()
                .justify_between()
                .pr_1()
                .child(section_header("Projects", cx))
                .child(
                    Button::new("new-project")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::FolderPlus)
                        .tooltip("New project")
                        .on_click(cx.listener(|this, _, window, cx| this.new_project(window, cx))),
                )
                .into_any_element(),
        );
        if self.projects.is_empty() {
            sections.push(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Group chats with shared instructions and files.")
                    .into_any_element(),
            );
        }
        for project in &self.projects {
            let chats: Vec<&Chat> = self
                .chats
                .iter()
                .filter(|c| c.project.as_deref() == Some(&project.id) && !c.pinned)
                .filter(|c| matches(c, &filter))
                .collect();
            if filtering && chats.is_empty() {
                continue;
            }
            sections.push(self.render_project_row(&project.id, chats.len(), cx).into_any_element());
            if project.expanded || filtering {
                for chat in chats {
                    sections.push(self.render_chat_row(chat, 1, window, cx).into_any_element());
                }
            }
        }

        // the other chats, by date
        let today = Local::now().date_naive();
        let mut last_group = "";
        for chat in self
            .chats
            .iter()
            .filter(|c| c.project.is_none() && !c.pinned && matches(c, &filter))
        {
            let date = chat.updated.with_timezone(&Local).date_naive();
            let days = (today - date).num_days();
            let group = match days {
                ..=0 => "Today",
                1 => "Yesterday",
                2..=7 => "Previous 7 days",
                8..=30 => "Previous 30 days",
                _ => "Older",
            };
            if group != last_group {
                sections.push(section_header(group, cx).into_any_element());
                last_group = group;
            }
            sections.push(self.render_chat_row(chat, 0, window, cx).into_any_element());
        }

        v_flex()
            .h_full()
            .w(px(264.))
            .flex_shrink_0()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("Chats"))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("new-chat")
                                    .ghost()
                                    .small()
                                    .icon(Lucide::SquarePen)
                                    .tooltip("New chat (⌘N)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_chat(None, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("hide-chats")
                                    .ghost()
                                    .small()
                                    .icon(IconName::PanelLeftClose)
                                    .tooltip("Hide chats (⌘B)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.show_chat_list = false;
                                        this.save_settings();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div().px_2().pb_2().child(
                    Input::new(&self.chat_filter)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).size_3p5()),
                ),
            )
            .child(
                v_flex()
                    .id("chat-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .gap_px()
                    .children(sections),
            )
    }

    fn render_project_row(&self, id: &Id, count: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let Some(project) = self.project(id) else {
            return div().into_any_element();
        };
        let expanded = project.expanded;
        let active = self.right_panel == super::RightPanel::Project(id.clone());
        let weak = cx.entity().downgrade();
        let (id1, id2, id3, id4) = (id.clone(), id.clone(), id.clone(), id.clone());
        let name = project.name.clone();

        h_flex()
            .id(SharedString::from(format!("project-{id}")))
            .group("project-row")
            .h(px(30.))
            .px_1()
            .gap_1()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(active, |d| d.bg(theme.sidebar_accent))
            .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(&id1, cx)))
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size_3p5()
                .text_color(theme.muted_foreground),
            )
            .child(
                Icon::new(if expanded {
                    IconName::FolderOpen
                } else {
                    IconName::FolderClosed
                })
                .size_4()
                .text_color(theme.primary),
            )
            .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .group_hover("project-row", |s| s.invisible())
                    .child(count.to_string()),
            )
            .child(
                h_flex()
                    .invisible()
                    .group_hover("project-row", |s| s.visible())
                    .child(
                        Button::new(SharedString::from(format!("project-new-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New chat in project")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.new_chat(Some(id2.clone()), window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("project-menu-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Ellipsis)
                            .dropdown_menu(move |menu, _, _| {
                                let (w1, w2) = (weak.clone(), weak.clone());
                                let (i3, i4) = (id3.clone(), id4.clone());
                                menu.item(
                                    PopupMenuItem::new("Project settings")
                                        .icon(IconName::Settings)
                                        .on_click(move |_, window, cx| {
                                            let _ = w1.update(cx, |this, cx| {
                                                this.open_project_settings(i3.clone(), window, cx)
                                            });
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Delete project")
                                        .icon(Lucide::Trash)
                                        .on_click(move |_, window, cx| {
                                            confirm_delete_project(w2.clone(), i4.clone(), window, cx)
                                        }),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_chat_row(&self, chat: &Chat, indent: usize, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let active = self.active_chat.as_deref() == Some(&chat.id);
        let generating = self
            .generation
            .as_ref()
            .is_some_and(|g| g.chat == chat.id);
        let id = chat.id.clone();
        let weak = cx.entity().downgrade();
        let projects: Vec<(Id, String)> = self
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let in_project = chat.project.clone();
        let pinned = chat.pinned;
        let title = chat.title.clone();
        let select_id = id.clone();

        h_flex()
            .id(SharedString::from(format!("chat-{id}")))
            .group("chat-row")
            .h(px(30.))
            .pl(px(8. + indent as f32 * 18.))
            .pr_1()
            .gap_2()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(active, |d| {
                d.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
            })
            .when(!active, |d| d.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6))))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_chat(select_id.clone(), window, cx)
            }))
            .when(pinned, |d| {
                d.child(Icon::from(Lucide::Pin).size_3().text_color(theme.muted_foreground))
            })
            .child(div().flex_1().min_w_0().truncate().text_sm().child(title.clone()))
            .when(generating, |d| {
                d.child(div().size_1p5().rounded_full().bg(theme.primary))
            })
            .child(
                div()
                    .invisible()
                    .group_hover("chat-row", |s| s.visible())
                    .when(active, |d| d.visible())
                    .child(
                        Button::new(SharedString::from(format!("chat-menu-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Ellipsis)
                            .dropdown_menu(move |mut menu, _, _| {
                                let w = weak.clone();
                                let id = id.clone();
                                let action = |w: &WeakEntity<Studio>, id: &Id, f: fn(&mut Studio, &Id, &mut Window, &mut Context<Studio>)| {
                                    let (w, id) = (w.clone(), id.clone());
                                    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                                        let _ = w.update(cx, |this, cx| f(this, &id, window, cx));
                                    }
                                };
                                menu = menu
                                    .item(PopupMenuItem::new("Rename").icon(Lucide::Pencil).on_click({
                                        let (w, id, title) = (w.clone(), id.clone(), title.clone());
                                        move |_, window, cx| rename_dialog(w.clone(), id.clone(), title.clone(), window, cx)
                                    }))
                                    .item(
                                        PopupMenuItem::new(if pinned { "Unpin" } else { "Pin" })
                                            .icon(if pinned { Lucide::PinOff } else { Lucide::Pin })
                                            .on_click(action(&w, &id, |this, id, _, cx| this.toggle_pin(id, cx))),
                                    )
                                    .item(
                                        PopupMenuItem::new("Duplicate")
                                            .icon(IconName::Copy)
                                            .on_click(action(&w, &id, |this, id, window, cx| {
                                                this.duplicate_chat(id, None, window, cx)
                                            })),
                                    )
                                    .item(
                                        PopupMenuItem::new("Export as Markdown")
                                            .icon(Lucide::Upload)
                                            .on_click(action(&w, &id, |this, id, window, cx| {
                                                this.export_chat(id, window, cx)
                                            })),
                                    );
                                if !projects.is_empty() || in_project.is_some() {
                                    menu = menu.separator().label("Move to project");
                                    for (pid, pname) in &projects {
                                        let (w, id, pid) = (w.clone(), id.clone(), pid.clone());
                                        let here = in_project.as_deref() == Some(&pid);
                                        menu = menu.item(
                                            PopupMenuItem::new(pname.clone())
                                                .icon(IconName::Folder)
                                                .checked(here)
                                                .on_click(move |_, _, cx| {
                                                    let _ = w.update(cx, |this, cx| {
                                                        this.move_chat(&id, Some(pid.clone()), cx)
                                                    });
                                                }),
                                        );
                                    }
                                    if in_project.is_some() {
                                        let (w, id) = (w.clone(), id.clone());
                                        menu = menu.item(
                                            PopupMenuItem::new("Remove from project")
                                                .icon(IconName::Minus)
                                                .on_click(move |_, _, cx| {
                                                    let _ = w.update(cx, |this, cx| {
                                                        this.move_chat(&id, None, cx)
                                                    });
                                                }),
                                        );
                                    }
                                }
                                menu.separator().item(
                                    PopupMenuItem::new("Delete")
                                        .icon(Lucide::Trash)
                                        .on_click(action(&w, &id, |this, id, window, cx| {
                                            this.delete_chat(id.clone(), window, cx)
                                        })),
                                )
                            }),
                    ),
            )
    }
}

fn section_header(text: &'static str, cx: &App) -> impl IntoElement + use<> {
    div().px_2().pt_3().pb_1().child(section_label(text, cx))
}

fn rename_dialog(this: WeakEntity<Studio>, id: Id, title: String, window: &mut Window, cx: &mut App) {
    let Some(studio) = this.upgrade() else {
        return;
    };
    let input = studio.read(cx).rename_input.clone();
    input.update(cx, |i, cx| {
        i.set_value(title, window, cx);
        i.focus(window, cx);
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let (this, id, input) = (this.clone(), id.clone(), input.clone());
        let ok = {
            let (this, id, input) = (this.clone(), id.clone(), input.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let value = input.read(cx).value().to_string();
                let _ = this.update(cx, |this, cx| this.rename_chat(&id, value, cx));
                window.close_dialog(cx);
            }
        };
        dialog
            .title("Rename chat")
            .w(px(420.))
            .child(Input::new(&input))
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("rename-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("rename-ok").primary().label("Rename").on_click(ok)),
            )
    });
}

fn confirm_delete_project(this: WeakEntity<Studio>, id: Id, window: &mut Window, cx: &mut App) {
    let name = this
        .upgrade()
        .and_then(|s| s.read(cx).project(&id).map(|p| p.name.clone()))
        .unwrap_or_default();
    window.open_dialog(cx, move |dialog, _, _| {
        let (this, id) = (this.clone(), id.clone());
        dialog
            .title(format!("Delete project “{name}”?"))
            .w(px(440.))
            .child(
                div()
                    .text_sm()
                    .child("Its chats are kept and moved out of the project. Its instructions and files are deleted."),
            )
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("del-project-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("del-project-ok")
                            .danger()
                            .label("Delete")
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |this, cx| this.delete_project(&id, cx));
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

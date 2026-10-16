//! The chat page: the chat list, the conversation and its composer, and the
//! settings panel.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
    relative,
};
use gpui_component::{Disableable as _, StyledExt as _, WindowExt as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    message_scroller::MessageScroller,
    spinner::Spinner,
    text::TextView,
    v_flex,
};

use super::{ModelState, Studio};
use crate::assets::Lucide;
use crate::conversation;
use crate::store::{ChatMessage, Role};

const CONTENT_WIDTH: f32 = 780.;

impl Studio {
    pub fn render_chat_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .size_full()
            .when(self.settings.show_chat_list, |d| {
                d.child(self.render_chat_list(window, cx))
            })
            .child(self.render_conversation(window, cx))
            .when(self.settings.show_right_panel, |d| {
                d.child(self.render_settings_panel(window, cx))
            })
    }

    fn render_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let chat = self.active();
        let title: SharedString = chat.map(|c| c.title.clone()).unwrap_or_default().into();
        let project: Option<SharedString> = chat
            .and_then(|c| self.project_of(c))
            .map(|p| p.name.clone().into());
        let empty = chat.is_none_or(|c| c.messages.is_empty());
        let chat_id = chat.map(|c| c.id.clone());

        let header = h_flex()
            .h(px(44.))
            .px_3()
            .gap_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme.border)
            .when(!self.settings.show_chat_list, |d| {
                d.child(
                    Button::new("show-chats")
                        .ghost()
                        .small()
                        .icon(IconName::PanelLeftOpen)
                        .tooltip("Show chats (⌘B)")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.show_chat_list = true;
                            this.save_settings();
                            cx.notify();
                        })),
                )
            })
            .when_some(project, |d, p| {
                d.child(
                    h_flex()
                        .gap_1()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(Icon::new(IconName::Folder).size_3p5())
                        .child(p)
                        .child("/"),
                )
            })
            .child(div().flex_1().min_w_0().truncate().text_sm().font_medium().child(title))
            .when_some(chat_id.clone(), |d, id| {
                d.child(
                    Button::new("export")
                        .ghost()
                        .small()
                        .icon(Lucide::Upload)
                        .tooltip("Export as Markdown")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.export_chat(&id, window, cx)
                        })),
                )
            });

        let body: AnyElement = if empty {
            self.render_empty_chat(cx).into_any_element()
        } else {
            let renderer = cx.processor(|this: &mut Self, ix: usize, window, cx| {
                this.render_message(ix, window, cx)
            });
            MessageScroller::new("messages", self.scroller.clone(), renderer)
                .with_bottom_fade(theme.background)
                .flex_1()
                .min_h_0()
                .into_any_element()
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .child(self.render_composer(window, cx))
    }

    fn render_empty_chat(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let (headline, sub): (SharedString, SharedString) = match &self.model_state {
            ModelState::Loaded(m) => (
                m.name.clone().into(),
                "Send a message to start the conversation.".into(),
            ),
            ModelState::Loading { name, .. } => {
                (format!("Loading {name}…").into(), "This takes a few seconds.".into())
            }
            ModelState::Unloaded => (
                "No model loaded".into(),
                "Pick a model to load with ⌘L, or find one in Discover.".into(),
            ),
        };
        let project_hint: Option<SharedString> = self
            .active()
            .and_then(|c| self.project_of(c))
            .map(|p| {
                let files = p.files.len();
                format!(
                    "In project {}{}",
                    p.name,
                    if files > 0 {
                        format!(" · {files} reference file{}", if files == 1 { "" } else { "s" })
                    } else {
                        String::new()
                    }
                )
                .into()
            });
        let unloaded = matches!(self.model_state, ModelState::Unloaded);

        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::from(Lucide::Sparkles)
                    .size(px(40.))
                    .text_color(theme.primary.opacity(0.8)),
            )
            .child(div().text_xl().font_semibold().child(headline))
            .child(div().text_sm().text_color(theme.muted_foreground).child(sub))
            .when_some(project_hint, |d, h| {
                d.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(Icon::new(IconName::Folder).size_3())
                        .child(h),
                )
            })
            .when(unloaded, |d| {
                d.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("empty-load")
                                .primary()
                                .icon(Lucide::Box)
                                .label("Select a model")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_picker(window, cx)
                                })),
                        )
                        .child(
                            Button::new("empty-discover")
                                .outline()
                                .icon(IconName::Search)
                                .label("Discover")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_page(super::Page::Discover, window, cx)
                                })),
                        ),
                )
            })
    }

    fn render_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.active().and_then(|c| c.messages.get(ix)).cloned() else {
            return div().into_any_element();
        };
        let row = match message.role {
            Role::User => self.render_user_message(&message, window, cx).into_any_element(),
            Role::Assistant => self
                .render_assistant_message(ix, &message, window, cx)
                .into_any_element(),
            Role::System => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(message.content.clone())
                .into_any_element(),
        };
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .py_3()
            .child(div().w_full().max_w(px(CONTENT_WIDTH)).child(row))
            .into_any_element()
    }

    fn render_user_message(&mut self, message: &ChatMessage, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let id = message.id.clone();

        if let Some((editing_id, input)) = &self.editing {
            if editing_id == &id {
                return v_flex()
                    .w_full()
                    .gap_2()
                    .child(Textarea::new(input))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("edit-cancel")
                                    .small()
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                            )
                            .child(
                                Button::new("edit-save")
                                    .small()
                                    .outline()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.commit_edit(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("edit-submit")
                                    .small()
                                    .primary()
                                    .label("Save & Submit")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.commit_edit(true, window, cx)
                                    })),
                            ),
                    )
                    .into_any_element();
            }
        }

        let (answer, _) = self.message_views(message, cx);
        let content = message.content.clone();
        let (id1, id2, id3) = (id.clone(), id.clone(), id.clone());

        v_flex()
            .id(SharedString::from(format!("msg-{id}")))
            .group("message")
            .w_full()
            .items_end()
            .gap_1()
            .when(!message.attachments.is_empty(), |d| {
                d.child(
                    h_flex().gap_1().flex_wrap().justify_end().children(
                        message.attachments.iter().map(|a| {
                            h_flex()
                                .gap_1()
                                .px_2()
                                .py_1()
                                .rounded(theme.radius)
                                .border_1()
                                .border_color(theme.border)
                                .text_xs()
                                .child(Icon::new(IconName::FileText).size_3p5())
                                .child(a.name.clone())
                        }),
                    ),
                )
            })
            .child(
                div()
                    .max_w(relative(0.85))
                    .px_4()
                    .py_2p5()
                    .rounded(px(16.))
                    .bg(theme.secondary)
                    .text_color(theme.secondary_foreground)
                    .child(TextView::new(&answer).selectable(true)),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .invisible()
                    .group_hover("message", |s| s.visible())
                    .child(
                        Button::new("user-copy")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .tooltip("Copy")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.copy(content.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("user-edit")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Pencil)
                            .tooltip("Edit")
                            .disabled(self.is_generating())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_edit(&id1, window, cx)
                            })),
                    )
                    .child(
                        Button::new("user-branch")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::GitBranch)
                            .tooltip("Branch from here")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(chat) = this.active_chat.clone() {
                                    this.duplicate_chat(&chat, Some(&id2), window, cx)
                                }
                            })),
                    )
                    .child(
                        Button::new("user-delete")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Trash)
                            .tooltip("Delete")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_message(&id3, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_assistant_message(
        &mut self,
        ix: usize,
        message: &ChatMessage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let id = message.id.clone();
        let streaming = self
            .generation
            .as_ref()
            .is_some_and(|g| g.message == message.id);
        let is_last = self
            .active()
            .is_some_and(|c| c.messages.len() == ix + 1);

        if let Some((editing_id, input)) = &self.editing {
            if editing_id == &id {
                return v_flex()
                    .w_full()
                    .gap_2()
                    .child(Textarea::new(input))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("edit-cancel")
                                    .small()
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                            )
                            .child(
                                Button::new("edit-save")
                                    .small()
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.commit_edit(false, window, cx)
                                    })),
                            ),
                    )
                    .into_any_element();
            }
        }

        let (answer, reasoning) = self.message_views(message, cx);
        let reply = conversation::split_reasoning(&message.content);
        let has_reasoning = reply.reasoning.is_some_and(|r| !r.is_empty());
        let thinking = streaming && reply.reasoning.is_some() && !reply.reasoning_done;
        let expanded = self.expanded_reasoning.contains(&id) || thinking;
        let answer_empty = reply.answer.trim().is_empty();
        let copy_text = reply.answer.to_string();
        let waiting = streaming && message.content.is_empty();
        let prompt_processing = streaming
            && self
                .generation
                .as_ref()
                .is_some_and(|g| g.prompt_done.is_none());

        let reasoning_label: SharedString = if thinking {
            "Thinking…".into()
        } else if let Some(ms) = message.reasoning_ms {
            format!("Thought for {:.1} seconds", ms as f32 / 1000.0).into()
        } else {
            "Thoughts".into()
        };

        let model = message.model.clone().unwrap_or_else(|| "Assistant".into());
        let (id1, id2, id3, id4) = (id.clone(), id.clone(), id.clone(), id.clone());

        v_flex()
            .id(SharedString::from(format!("msg-{id}")))
            .group("message")
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Icon::from(Lucide::Sparkles).size_3p5().text_color(theme.primary))
                    .child(div().font_medium().child(model)),
            )
            .when(waiting, |d| {
                d.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(Spinner::new().small())
                        .child(if prompt_processing {
                            "Processing prompt…"
                        } else {
                            "Generating…"
                        }),
                )
            })
            .when(has_reasoning || thinking, |d| {
                d.child(
                    v_flex()
                        .w_full()
                        .rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            h_flex()
                                .id(SharedString::from(format!("reasoning-{id}")))
                                .px_3()
                                .py_1p5()
                                .gap_2()
                                .cursor_pointer()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_reasoning(&id1, cx)
                                }))
                                .child(if thinking {
                                    Spinner::new().small().into_any_element()
                                } else {
                                    Icon::from(Lucide::Lightbulb).size_4().into_any_element()
                                })
                                .child(div().flex_1().child(reasoning_label))
                                .child(
                                    Icon::new(if expanded {
                                        IconName::ChevronUp
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .size_4(),
                                ),
                        )
                        .when(expanded, |d| {
                            d.child(
                                div()
                                    .px_3()
                                    .pb_2()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .border_t_1()
                                    .border_color(theme.border)
                                    .pt_2()
                                    .child(TextView::new(&reasoning).selectable(true).stream_fade(streaming)),
                            )
                        }),
                )
            })
            .when(!answer_empty, |d| {
                d.child(
                    TextView::new(&answer)
                        .selectable(true)
                        .stream_fade(streaming)
                        .code_block_actions(|block, _, _| {
                            let code = block.code().to_string();
                            let key = format!("copy-code-{:x}", fxhash(&code));
                            Button::new(SharedString::from(key))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .tooltip("Copy code")
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(code.clone()));
                                    window.push_notification(
                                        gpui_component::notification::Notification::info("Copied to clipboard"),
                                        cx,
                                    );
                                })
                        }),
                )
            })
            .when_some(message.error.clone(), |d, e| {
                d.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.danger)
                        .text_sm()
                        .text_color(theme.danger)
                        .child(Icon::new(IconName::CircleX).size_4())
                        .child(div().flex_1().child(e)),
                )
            })
            .when(!streaming, |d| {
                d.child(
                    h_flex()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_0p5()
                                .child(
                                    Button::new("asst-copy")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Copy)
                                        .tooltip("Copy")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.copy(copy_text.clone(), window, cx)
                                        })),
                                )
                                .when(is_last, |d| {
                                    d.child(
                                        Button::new("asst-regenerate")
                                            .ghost()
                                            .xsmall()
                                            .icon(Lucide::RotateCcw)
                                            .tooltip("Regenerate")
                                            .disabled(self.is_generating() || self.model_state.loaded().is_none())
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.regenerate(&id2, window, cx)
                                            })),
                                    )
                                })
                                .child(
                                    Button::new("asst-edit")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Pencil)
                                        .tooltip("Edit")
                                        .disabled(self.is_generating())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.start_edit(&id3, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("asst-branch")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::GitBranch)
                                        .tooltip("Branch from here")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if let Some(chat) = this.active_chat.clone() {
                                                this.duplicate_chat(&chat, Some(&id), window, cx)
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("asst-delete")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Trash)
                                        .tooltip("Delete")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_message(&id4, cx)
                                        })),
                                ),
                        )
                        .when_some(message.stats.clone(), |d, s| {
                            d.child(
                                h_flex()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{:.2} tok/sec", s.tokens_per_sec))
                                    .child(format!("{} tokens", s.tokens))
                                    .child(format!(
                                        "{:.2}s to first token",
                                        s.time_to_first_token_ms as f32 / 1000.0
                                    ))
                                    .child(format!("Stop reason: {}", s.stop_reason.label())),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn render_composer(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let generating = self.is_generating();
        let usage = self.context_usage();
        let can_send = self.model_state.loaded().is_some();

        h_flex().w_full().justify_center().px_6().pb_4().pt_1().child(
            v_flex()
                .w_full()
                .max_w(px(CONTENT_WIDTH))
                .gap_1()
                .p_2()
                .rounded(px(14.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary.opacity(0.5))
                .when(!self.pending_attachments.is_empty(), |d| {
                    d.child(
                        h_flex().gap_1().flex_wrap().px_1().children(
                            self.pending_attachments.iter().enumerate().map(|(ix, a)| {
                                h_flex()
                                    .gap_1()
                                    .pl_2()
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.background)
                                    .text_xs()
                                    .child(Icon::new(IconName::FileText).size_3p5())
                                    .child(a.name.clone())
                                    .child(
                                        Button::new(("remove-attachment", ix))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Close)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if ix < this.pending_attachments.len() {
                                                    this.pending_attachments.remove(ix);
                                                }
                                                cx.notify();
                                            })),
                                    )
                            }),
                        ),
                    )
                })
                .child(
                    Textarea::new(&self.composer)
                        .appearance(false)
                        .bordered(false),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("attach")
                                .ghost()
                                .small()
                                .icon(Lucide::Paperclip)
                                .tooltip("Attach text files")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.attach_files(window, cx)
                                })),
                        )
                        .when_some(usage, |d, (used, size)| {
                            let pct = used as f32 / size.max(1) as f32 * 100.0;
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(if pct > 90.0 {
                                        theme.danger
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(format!(
                                        "Context is {pct:.1}% full ({used} / {size} tokens)"
                                    )),
                            )
                        })
                        .child(div().flex_1())
                        .when(!can_send && !generating, |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("No model loaded"),
                            )
                        })
                        .child(if generating {
                            Button::new("stop")
                                .small()
                                .danger()
                                .icon(Lucide::CircleStop)
                                .label("Stop")
                                .on_click(cx.listener(|this, _, _, cx| this.stop_generation(cx)))
                        } else {
                            Button::new("send")
                                .small()
                                .primary()
                                .icon(Lucide::Send)
                                .label("Send")
                                .tooltip("Send (⏎) · new line (⇧⏎)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_message(window, cx)
                                }))
                        }),
                ),
        )
    }

    /// (used, size) tokens of the context window, from the last generation
    fn context_usage(&self) -> Option<(u32, u32)> {
        let chat = self.active()?;
        let size = self.model_state.loaded()?.context_length;
        if let Some(g) = self.generation.as_ref().filter(|g| g.chat == chat.id) {
            if g.prompt_tokens > 0 {
                return Some((g.prompt_tokens + g.chunks, size));
            }
        }
        chat.messages
            .iter()
            .rev()
            .find_map(|m| m.stats.as_ref())
            .map(|s| (s.prompt_tokens + s.tokens, size))
    }
}

/// A small stable hash for element ids
fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

//! Discover: search GGUF models on the Hugging Face Hub, see a repository's
//! quantizations and model card, download them; or pull by name.

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, StatefulInteractiveElement as _, Styled, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, StyledExt as _};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    spinner::Spinner,
    text::{TextView, TextViewState},
    v_flex,
};
use skelm_hf::{ModelSummary, RepoInfo};

use super::util::{chip, date_of, human_count, human_size, section_label};
use super::{DownloadStatus, Studio};
use crate::assets::Lucide;
use crate::hub::{self, SearchSort};

#[derive(Default)]
pub struct DiscoverState {
    pub sort: SearchSort,
    pub results: Vec<ModelSummary>,
    pub searching: bool,
    pub searched_once: bool,
    pub error: Option<String>,
    pub selected: Option<String>,
    pub repo: Option<Result<RepoInfo, String>>,
    pub card: Option<Result<Entity<TextViewState>, String>>,
    generation: u64,
    search_task: Option<Task<()>>,
    repo_task: Option<Task<()>>,
    card_task: Option<Task<()>>,
}

impl Studio {
    pub fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.discover_search.read(cx).value().to_string();
        let sort = self.discover.sort;
        let d = &mut self.discover;
        d.generation += 1;
        let generation = d.generation;
        d.searching = true;
        d.searched_once = true;
        d.error = None;
        d.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = crate::runtime::spawn(hub::search(query, sort, 60))
                .await
                .and_then(|r| r);
            let _ = this.update_in(cx, |this, window, cx| {
                if this.discover.generation != generation {
                    return;
                }
                this.discover.searching = false;
                match result {
                    Ok(results) => {
                        let first = results.first().map(|m| m.id.clone());
                        this.discover.results = results;
                        if this.discover.selected.is_none() {
                            if let Some(first) = first {
                                this.select_repo(first, window, cx);
                            }
                        }
                    }
                    Err(e) => this.discover.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_repo(&mut self, repo: String, window: &mut Window, cx: &mut Context<Self>) {
        let d = &mut self.discover;
        if d.selected.as_deref() == Some(&repo) {
            return;
        }
        d.selected = Some(repo.clone());
        d.repo = None;
        d.card = None;
        let r = repo.clone();
        d.repo_task = Some(cx.spawn_in(window, async move |this, cx| {
            let info = crate::runtime::spawn(hub::repo_info(r.clone()))
                .await
                .and_then(|r| r)
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                if this.discover.selected.as_deref() == Some(&r) {
                    this.discover.repo = Some(info);
                    cx.notify();
                }
            });
        }));
        let r = repo;
        d.card_task = Some(cx.spawn_in(window, async move |this, cx| {
            let card = crate::runtime::spawn(hub::model_card(r.clone()))
                .await
                .and_then(|r| r)
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                if this.discover.selected.as_deref() == Some(&r) {
                    this.discover.card = Some(card.map(|text| cx.new(|cx| TextViewState::markdown(&text, cx))));
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub fn render_discover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let sort = self.discover.sort;
        let sort_button = |id: &'static str, label: &'static str, value: SearchSort, cx: &mut Context<Self>| {
            Button::new(id)
                .xsmall()
                .label(label)
                .when(sort == value, |b| b.primary())
                .when(sort != value, |b| b.ghost())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.discover.sort = value;
                    this.run_search(window, cx);
                }))
        };

        let results = v_flex()
            .id("discover-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .gap_0p5()
            .children(self.discover.results.iter().enumerate().map(|(ix, m)| {
                let selected = self.discover.selected.as_deref() == Some(&m.id);
                let (owner, name) = m.id.split_once('/').unwrap_or(("", &m.id));
                let id = m.id.clone();
                let date = m.last_modified.as_deref().map(date_of).unwrap_or_default();
                v_flex()
                    .id(("discover-result", ix))
                    .px_2()
                    .py_1p5()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.accent))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.accent.opacity(0.4))))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_repo(id.clone(), window, cx)
                    }))
                    .child(div().text_sm().font_medium().truncate().child(name.to_string()))
                    .child(
                        h_flex()
                            .gap_3()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(div().truncate().child(owner.to_string()))
                            .child(super::util::icon_label(Lucide::Download, human_count(m.downloads), true, cx))
                            .child(super::util::icon_label(IconName::Heart, human_count(m.likes), true, cx))
                            .child(date),
                    )
            }))
            .when(self.discover.searching, |d| {
                d.child(
                    h_flex()
                        .justify_center()
                        .py_6()
                        .child(Spinner::new()),
                )
            })
            .when_some(self.discover.error.clone(), |d, e| {
                d.child(div().p_2().text_sm().text_color(theme.danger).child(e))
            })
            .when(
                !self.discover.searching
                    && self.discover.error.is_none()
                    && self.discover.results.is_empty()
                    && self.discover.searched_once,
                |d| {
                    d.child(
                        div()
                            .p_4()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("No GGUF repository found"),
                    )
                },
            );

        let left = v_flex()
            .w(px(360.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .child(div().text_lg().font_semibold().child("Discover"))
                    .child(
                        Input::new(&self.discover_search)
                            .small()
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).size_3p5()),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(sort_button("sort-trending", "Trending", SearchSort::Trending, cx))
                            .child(sort_button("sort-downloads", "Downloads", SearchSort::Downloads, cx))
                            .child(sort_button("sort-likes", "Likes", SearchSort::Likes, cx))
                            .child(sort_button("sort-recent", "Recent", SearchSort::LastModified, cx)),
                    ),
            )
            .child(results)
            .child(
                v_flex()
                    .p_3()
                    .gap_1p5()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(section_label("PULL BY NAME", cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.pull_input).small()))
                            .child(
                                Button::new("pull")
                                    .small()
                                    .primary()
                                    .icon(Lucide::Download)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let name = this.pull_input.read(cx).value().trim().to_string();
                                        if !name.is_empty() {
                                            this.start_download(name, None, window, cx);
                                            this.pull_input.update(cx, |i, cx| i.set_value("", window, cx));
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Ollama registry models (qwen2.5:7b) or Hugging Face GGUFs (hf.co/owner/repo:Q4_K_M)"),
                    ),
            );

        h_flex()
            .size_full()
            .child(left)
            .child(self.render_repo_details(window, cx))
    }

    fn render_repo_details(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let Some(repo_id) = self.discover.selected.clone() else {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child(Icon::from(Lucide::Telescope).size(px(40.)))
                .child("Search for a model to see its details")
                .into_any_element();
        };
        let summary = self.discover.results.iter().find(|m| m.id == repo_id).cloned();
        let hf_url = format!("https://huggingface.co/{repo_id}");

        let quants: gpui::AnyElement = match &self.discover.repo {
            None => h_flex()
                .gap_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(Spinner::new().small())
                .child("Loading the repository…")
                .into_any_element(),
            Some(Err(e)) => div().text_sm().text_color(theme.danger).child(e.clone()).into_any_element(),
            Some(Ok(info)) => {
                let variants = info.gguf_variants();
                let projectors = skelm_hf::gguf::projectors(&info.files).len();
                v_flex()
                    .gap_1()
                    .when(info.gated, |d| {
                        d.child(
                            h_flex()
                                .gap_2()
                                .p_2()
                                .rounded(theme.radius)
                                .border_1()
                                .border_color(theme.warning)
                                .text_sm()
                                .child(Icon::new(IconName::TriangleAlert).size_4().text_color(theme.warning))
                                .child("Gated repository: accept its license on the Hub and set HF_TOKEN to download it."),
                        )
                    })
                    .when(variants.is_empty(), |d| {
                        d.child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("This repository has no GGUF file."),
                        )
                    })
                    .children(variants.iter().enumerate().map(|(ix, v)| {
                        let name = format!("hf.co/{repo_id}:{}", v.quant);
                        let installed = self.local_models.iter().any(|m| m.name == name);
                        let downloading = self.downloads.iter().any(|d| {
                            d.name == name && matches!(d.status, DownloadStatus::Running)
                        });
                        let recommended = v.quant == skelm_hf::gguf::DEFAULT_QUANT;
                        let size = v.size();
                        let shards = v.files.len();
                        h_flex()
                            .px_3()
                            .py_2()
                            .gap_3()
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(if recommended { theme.primary.opacity(0.6) } else { theme.border })
                            .child(
                                h_flex()
                                    .flex_1()
                                    .gap_2()
                                    .child(div().text_sm().font_medium().child(v.quant.clone()))
                                    .when(recommended, |d| d.child(chip("Recommended", cx)))
                                    .when(shards > 1, |d| d.child(chip(format!("{shards} files"), cx))),
                            )
                            .child(div().text_sm().text_color(theme.muted_foreground).child(human_size(size)))
                            .child(if installed {
                                Button::new(("quant-installed", ix))
                                    .small()
                                    .ghost()
                                    .icon(IconName::Check)
                                    .label("Downloaded")
                                    .disabled(true)
                            } else if downloading {
                                Button::new(("quant-downloading", ix))
                                    .small()
                                    .ghost()
                                    .loading(true)
                                    .label("Downloading")
                                    .disabled(true)
                            } else {
                                Button::new(("quant-download", ix))
                                    .small()
                                    .when(recommended, |b| b.primary())
                                    .when(!recommended, |b| b.outline())
                                    .icon(Lucide::Download)
                                    .label("Download")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.start_download(name.clone(), Some(size), window, cx)
                                    }))
                            })
                    }))
                    .when(projectors > 0, |d| {
                        d.child(
                            div()
                                .pt_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Includes a multimodal projector (vision is not supported yet, the text model works)."),
                        )
                    })
                    .into_any_element()
            }
        };

        let card: gpui::AnyElement = match &self.discover.card {
            None => h_flex()
                .gap_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(Spinner::new().small())
                .child("Loading the model card…")
                .into_any_element(),
            Some(Err(e)) => div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(e.clone())
                .into_any_element(),
            Some(Ok(state)) => TextView::new(state).selectable(true).into_any_element(),
        };

        let (owner, name) = repo_id.split_once('/').unwrap_or(("", &repo_id));
        v_flex()
            .id("repo-details")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(900.))
                    .p_6()
                    .gap_5()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xl().font_semibold().child(name.to_string()))
                                    .child(
                                        Button::new("open-hf")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ExternalLink)
                                            .tooltip("Open on Hugging Face")
                                            .on_click(move |_, _, cx| cx.open_url(&hf_url)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_3()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(owner.to_string())
                                    .when_some(summary.clone(), |d, s| {
                                        d.child(super::util::icon_label(Lucide::Download, human_count(s.downloads), true, cx))
                                            .child(super::util::icon_label(IconName::Heart, human_count(s.likes), true, cx))
                                            .when_some(s.last_modified.clone(), |d, m| {
                                                d.child(format!("Updated {}", date_of(&m)))
                                            })
                                    }),
                            )
                            .when_some(summary, |d, s| {
                                let tags: Vec<String> = s
                                    .tags
                                    .iter()
                                    .filter(|t| !t.contains(':') && t.as_str() != "gguf")
                                    .take(10)
                                    .cloned()
                                    .collect();
                                d.child(h_flex().pt_1().gap_1().flex_wrap().children(tags.into_iter().map(|t| chip(t, cx))))
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(section_label("DOWNLOAD OPTIONS", cx))
                            .child(quants),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(section_label("MODEL CARD", cx))
                            .child(
                                div()
                                    .p_4()
                                    .rounded(theme.radius_lg)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(card),
                            ),
                    ),
            )
            .into_any_element()
    }
}

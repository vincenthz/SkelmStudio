//! The main window. [`Studio`] owns the whole application state; each page
//! renders from it in its own module (`impl Studio` blocks).

mod chat;
mod chat_list;
mod developer;
mod discover;
mod downloads;
mod models;
mod picker;
mod settings_panel;
mod shell;
pub mod util;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, KeyBinding,
    PathPromptOptions, ScrollHandle, Subscription, Task, Window, actions,
};
use gpui_component::{
    Sizable as _, Theme, ThemeMode, WindowExt as _,
    button::Button,
    input::{InputEvent, InputState, TextareaState},
    message_scroller::MessageScrollerState,
    notification::Notification,
    slider::{SliderEvent, SliderState},
    text::TextViewState,
};

use crate::catalog::{self, LocalModel, ModelInfo};
use crate::conversation;
use crate::engine::{Command, Engine, Event, GenerateRequest, LoadedModel};
use crate::hub::DownloadProgress;
use crate::logs::{self, LogLine};
use crate::server::ServerHandle;
use crate::store::{
    Attachment, Chat, ChatMessage, Id, LoadConfig, Project, ProjectFile, Role, Sampling,
    Settings, Store,
};

pub use discover::DiscoverState;

actions!(
    studio,
    [
        OpenModelPicker,
        NewChat,
        ToggleChatList,
        ToggleSettingsPanel,
        EjectModel,
        StopGeneration,
        ClosePicker,
        Quit
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-l", OpenModelPicker, None),
        KeyBinding::new("cmd-n", NewChat, None),
        KeyBinding::new("cmd-b", ToggleChatList, None),
        KeyBinding::new("cmd-.", ToggleSettingsPanel, None),
        KeyBinding::new("cmd-e", EjectModel, None),
        KeyBinding::new("escape", ClosePicker, Some("ModelPicker")),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Chat,
    Developer,
    Models,
    Discover,
}

pub enum ModelState {
    Unloaded,
    Loading { name: String, since: Instant },
    Loaded(LoadedModel),
}

impl ModelState {
    pub fn loaded(&self) -> Option<&LoadedModel> {
        match self {
            ModelState::Loaded(m) => Some(m),
            _ => None,
        }
    }
}

/// The reply being generated
pub struct Generation {
    pub id: u64,
    pub chat: Id,
    pub message: Id,
    pub cancel: Arc<AtomicBool>,
    pub started: Instant,
    pub prompt_done: Option<Instant>,
    pub prompt_tokens: u32,
    pub chunks: u32,
}

/// The rendered markdown of a message, kept in sync with its content
pub struct MessageViews {
    pub answer: Entity<TextViewState>,
    pub reasoning: Entity<TextViewState>,
    answer_text: String,
    reasoning_text: String,
}

impl MessageViews {
    fn new(cx: &mut App) -> Self {
        Self {
            answer: cx.new(|cx| TextViewState::markdown("", cx)),
            reasoning: cx.new(|cx| TextViewState::markdown("", cx)),
            answer_text: String::new(),
            reasoning_text: String::new(),
        }
    }

    fn sync(&mut self, role: Role, content: &str, cx: &mut App) {
        let (answer, reasoning) = match role {
            Role::Assistant => {
                let reply = conversation::split_reasoning(content);
                (reply.answer, reply.reasoning.unwrap_or(""))
            }
            _ => (content, ""),
        };
        if self.answer_text != answer {
            self.answer_text = answer.to_string();
            self.answer.update(cx, |s, cx| s.set_text(answer, cx));
        }
        if self.reasoning_text != reasoning {
            self.reasoning_text = reasoning.to_string();
            self.reasoning.update(cx, |s, cx| s.set_text(reasoning, cx));
        }
    }
}

/// The sampling controls of the settings panel
pub struct SamplingSliders {
    pub temperature: Entity<SliderState>,
    pub top_k: Entity<SliderState>,
    pub top_p: Entity<SliderState>,
    pub min_p: Entity<SliderState>,
    pub repeat_penalty: Entity<SliderState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RightPanel {
    Settings,
    Project(Id),
}

pub enum DownloadStatus {
    Running,
    Done(String),
    Failed(String),
    Cancelled,
}

pub struct Download {
    pub id: u64,
    pub name: String,
    pub progress: Arc<DownloadProgress>,
    pub status: DownloadStatus,
    pub started: Instant,
    abort: Option<tokio::task::AbortHandle>,
}

pub struct Studio {
    pub store: Arc<Store>,
    pub settings: Settings,
    pub engine: Engine,
    pub focus: FocusHandle,
    pub page: Page,

    // models
    pub local_models: Vec<LocalModel>,
    pub model_state: ModelState,
    pub model_info: HashMap<String, Result<ModelInfo, String>>,
    inspecting: HashSet<String>,

    // chats
    pub projects: Vec<Project>,
    pub chats: Vec<Chat>,
    pub active_chat: Option<Id>,
    pub generation: Option<Generation>,
    next_request: u64,
    pub views: HashMap<Id, MessageViews>,
    pub expanded_reasoning: HashSet<Id>,
    pub scroller: Entity<MessageScrollerState>,
    scroller_chat: Option<Id>,
    pub editing: Option<(Id, Entity<TextareaState>)>,
    pub pending_attachments: Vec<Attachment>,
    pub right_panel: RightPanel,

    // inputs
    pub composer: Entity<TextareaState>,
    pub chat_filter: Entity<InputState>,
    pub system_prompt: Entity<TextareaState>,
    pub max_tokens: Entity<InputState>,
    pub seed: Entity<InputState>,
    pub sliders: SamplingSliders,
    pub rename_input: Entity<InputState>,
    pub project_name: Entity<InputState>,
    pub project_instructions: Entity<TextareaState>,

    // model picker
    pub picker: Option<picker::Picker>,
    pub picker_search: Entity<InputState>,

    // my models
    pub models_filter: Entity<InputState>,
    pub selected_model: Option<String>,

    // discover
    pub discover: DiscoverState,
    pub discover_search: Entity<InputState>,
    pub pull_input: Entity<InputState>,

    // downloads
    pub downloads: Vec<Download>,
    pub downloads_open: bool,
    next_download: u64,
    ticker: Option<Task<()>>,

    // developer
    pub server: Option<ServerHandle>,
    pub server_busy: bool,
    pub logs: VecDeque<LogLine>,
    pub logs_scroll: ScrollHandle,
    pub server_host: Entity<InputState>,
    pub server_port: Entity<InputState>,

    _subscriptions: Vec<Subscription>,
}

const MAX_LOG_LINES: usize = 2000;

impl Studio {
    pub fn new(
        store: Arc<Store>,
        logs_rx: async_channel::Receiver<LogLine>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = store.load_settings();
        let projects = store.load_projects();
        let chats = store.load_chats();
        let (engine, events) = Engine::spawn();

        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(true)
                .placeholder("Send a message to the model…")
        });
        let chat_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        let system_prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 12)
                .placeholder("You are a helpful assistant…")
        });
        let max_tokens = cx.new(|cx| InputState::new(window, cx).placeholder("No limit"));
        let seed = cx.new(|cx| InputState::new(window, cx).placeholder("Random"));
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let project_name = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        let project_instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 14)
                .placeholder("Instructions given to every chat of this project…")
        });
        let picker_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type to filter models…"));
        let models_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter models"));
        let discover_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search GGUF models on Hugging Face…")
        });
        let pull_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("qwen2.5:7b or hf.co/owner/repo:Q4_K_M")
        });
        let server_host =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.server_host.clone()));
        let server_port = cx.new(|cx| {
            InputState::new(window, cx).default_value(settings.server_port.to_string())
        });

        let s = &settings.sampling;
        let slider = |cx: &mut Context<Self>, min: f32, max: f32, step: f32, value: f32| {
            cx.new(|_| {
                SliderState::new()
                    .max(max)
                    .min(min)
                    .step(step)
                    .default_value(value)
            })
        };
        let sliders = SamplingSliders {
            temperature: slider(cx, 0.0, 2.0, 0.05, s.temperature),
            top_k: slider(cx, 0.0, 200.0, 1.0, s.top_k as f32),
            top_p: slider(cx, 0.0, 1.0, 0.01, s.top_p),
            min_p: slider(cx, 0.0, 1.0, 0.01, s.min_p),
            repeat_penalty: slider(cx, 1.0, 2.0, 0.01, s.repeat_penalty),
        };

        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));

        let mut subscriptions = vec![
            cx.subscribe_in(&composer, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    this.send_message(window, cx);
                }
            }),
            cx.subscribe_in(&system_prompt, window, |this, input, ev: &InputEvent, _, cx| {
                match ev {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        this.with_active_chat(cx, false, |chat| chat.system_prompt = value);
                    }
                    InputEvent::Blur => this.save_active_chat(cx),
                    _ => {}
                }
            }),
            cx.subscribe_in(&max_tokens, window, |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    let value = input.read(cx).value().trim().parse::<u32>().ok();
                    this.with_active_chat(cx, true, |chat| {
                        chat.sampling.max_tokens = value.filter(|v| *v > 0)
                    });
                }
            }),
            cx.subscribe_in(&seed, window, |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    let value = input.read(cx).value().trim().parse::<u32>().ok();
                    this.with_active_chat(cx, true, |chat| chat.sampling.seed = value);
                }
            }),
            cx.subscribe_in(&project_instructions, window, |this, input, ev: &InputEvent, _, cx| {
                if let (InputEvent::Change, RightPanel::Project(id)) = (ev, &this.right_panel) {
                    let id = id.clone();
                    let value = input.read(cx).value().to_string();
                    if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                        p.instructions = value;
                    }
                }
                if let InputEvent::Blur = ev {
                    this.save_projects();
                }
            }),
            cx.subscribe_in(&project_name, window, |this, input, ev: &InputEvent, _, cx| {
                if let (InputEvent::Change, RightPanel::Project(id)) = (ev, &this.right_panel) {
                    let id = id.clone();
                    let value = input.read(cx).value().trim().to_string();
                    if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                        if !value.is_empty() {
                            p.name = value;
                        }
                    }
                    cx.notify();
                }
                if let InputEvent::Blur = ev {
                    this.save_projects();
                }
            }),
            cx.subscribe_in(&discover_search, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.run_search(window, cx);
                }
            }),
            cx.subscribe_in(&pull_input, window, |this, input, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let name = input.read(cx).value().trim().to_string();
                    if !name.is_empty() {
                        this.start_download(name, None, window, cx);
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                    }
                }
            }),
            cx.subscribe(&picker_search, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.picker_load(cx);
                }
                cx.notify()
            }),
            cx.subscribe(&models_filter, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&chat_filter, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        for (slider, apply) in [
            (
                sliders.temperature.clone(),
                (|s: &mut Sampling, v: f32| s.temperature = v) as fn(&mut Sampling, f32),
            ),
            (sliders.top_k.clone(), |s, v| s.top_k = v.round() as u32),
            (sliders.top_p.clone(), |s, v| s.top_p = v),
            (sliders.min_p.clone(), |s, v| s.min_p = v),
            (sliders.repeat_penalty.clone(), |s, v| s.repeat_penalty = v),
        ] {
            subscriptions.push(cx.subscribe(&slider, move |this, _, ev: &SliderEvent, cx| {
                let (SliderEvent::Change(v) | SliderEvent::Release(v)) = ev;
                let save = matches!(ev, SliderEvent::Release(_));
                let value = (v.start() * 100.0).round() / 100.0;
                this.with_active_chat(cx, save, |chat| apply(&mut chat.sampling, value));
            }));
        }

        // inference engine events
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let mut batch = vec![event];
                while let Ok(e) = events.try_recv() {
                    batch.push(e);
                }
                if this
                    .update_in(cx, |this, window, cx| {
                        for e in batch {
                            this.on_engine_event(e, window, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // the log of the Developer page
        cx.spawn(async move |this, cx| {
            while let Ok(line) = logs_rx.recv().await {
                let mut batch = vec![line];
                while let Ok(l) = logs_rx.try_recv() {
                    batch.push(l);
                }
                if this
                    .update(cx, |this, cx| {
                        for line in batch {
                            if this.logs.len() == MAX_LOG_LINES {
                                this.logs.pop_front();
                            }
                            this.logs.push_back(line);
                        }
                        if this.page == Page::Developer {
                            this.logs_scroll.scroll_to_bottom();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut this = Self {
            store,
            engine,
            focus: cx.focus_handle(),
            page: Page::Chat,
            local_models: Vec::new(),
            model_state: ModelState::Unloaded,
            model_info: HashMap::new(),
            inspecting: HashSet::new(),
            projects,
            chats,
            active_chat: None,
            generation: None,
            next_request: 1,
            views: HashMap::new(),
            expanded_reasoning: HashSet::new(),
            scroller,
            scroller_chat: None,
            editing: None,
            pending_attachments: Vec::new(),
            right_panel: RightPanel::Settings,
            composer,
            chat_filter,
            system_prompt,
            max_tokens,
            seed,
            sliders,
            rename_input,
            project_name,
            project_instructions,
            picker: None,
            picker_search,
            models_filter,
            selected_model: None,
            discover: DiscoverState::default(),
            discover_search,
            pull_input,
            downloads: Vec::new(),
            downloads_open: false,
            next_download: 1,
            ticker: None,
            server: None,
            server_busy: false,
            logs: VecDeque::new(),
            logs_scroll: ScrollHandle::new(),
            server_host,
            server_port,
            settings,
            _subscriptions: subscriptions,
        };
        this.refresh_models(cx);
        match this.chats.first().map(|c| c.id.clone()) {
            Some(id) => this.select_chat(id, window, cx),
            None => this.new_chat(None, window, cx),
        }
        this.focus.focus(window, cx);
        this
    }

    // ---------------------------------------------------------------- state

    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.id == id)
    }

    pub fn chat_mut(&mut self, id: &str) -> Option<&mut Chat> {
        self.chats.iter_mut().find(|c| c.id == id)
    }

    pub fn active(&self) -> Option<&Chat> {
        self.active_chat.as_deref().and_then(|id| self.chat(id))
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn project_of(&self, chat: &Chat) -> Option<&Project> {
        chat.project.as_deref().and_then(|id| self.project(id))
    }

    pub fn is_generating(&self) -> bool {
        self.generation.is_some()
    }

    /// Change the active chat, then save it if `save`
    fn with_active_chat(&mut self, cx: &mut Context<Self>, save: bool, f: impl FnOnce(&mut Chat)) {
        let Some(id) = self.active_chat.clone() else {
            return;
        };
        if let Some(chat) = self.chat_mut(&id) {
            f(chat);
        }
        if save {
            self.save_chat(&id);
        }
        cx.notify();
    }

    pub fn save_chat(&self, id: &str) {
        if let Some(chat) = self.chat(id) {
            self.store.save_chat(chat);
        }
    }

    fn save_active_chat(&self, _cx: &mut Context<Self>) {
        if let Some(id) = &self.active_chat {
            self.save_chat(id);
        }
    }

    pub fn save_projects(&self) {
        self.store.save_projects(&self.projects);
    }

    pub fn save_settings(&self) {
        self.store.save_settings(&self.settings);
    }

    pub fn notify_error(&self, message: impl Into<String>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(message.into()), cx);
    }

    /// An error whose details are in the log, with a button to the Developer page
    pub fn notify_error_with_logs(
        &self,
        message: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let studio = cx.entity().downgrade();
        let notification = Notification::error(message.into()).action(move |_, _, cx| {
            let studio = studio.clone();
            let notification = cx.entity().downgrade();
            Button::new("view-logs")
                .small()
                .label("View logs")
                .on_click(move |_, window, cx| {
                    let _ = studio.update(cx, |s, cx| s.set_page(Page::Developer, window, cx));
                    let _ = notification.update(cx, |n, cx| n.dismiss(window, cx));
                })
        });
        window.push_notification(notification, cx);
    }

    pub fn notify_info(&self, message: impl Into<String>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::info(message.into()), cx);
    }

    pub fn notify_success(&self, message: impl Into<String>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(message.into()), cx);
    }

    pub fn copy(&self, text: impl Into<String>, window: &mut Window, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
        window.push_notification(Notification::info("Copied to clipboard"), cx);
    }

    pub fn set_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        match page {
            Page::Discover if !self.discover.searched_once => self.run_search(window, cx),
            Page::Models => self.refresh_models(cx),
            Page::Developer => self.logs_scroll.scroll_to_bottom(),
            _ => {}
        }
        cx.notify();
    }

    pub fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.dark_mode = !self.settings.dark_mode;
        let mode = if self.settings.dark_mode {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        Theme::change(mode, Some(window), cx);
        self.save_settings();
        cx.notify();
    }

    // ---------------------------------------------------------------- models

    pub fn refresh_models(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let models = crate::runtime::spawn_blocking(catalog::list_local)
                .await
                .unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.local_models = models;
                cx.notify();
            });
        })
        .detach();
    }

    /// Read a model's metadata in the background, once
    pub fn inspect_model(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.model_info.contains_key(name) || !self.inspecting.insert(name.to_string()) {
            return;
        }
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let n = name.clone();
            let info = crate::runtime::spawn_blocking(move || catalog::inspect(&n))
                .await
                .and_then(|r| r)
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.inspecting.remove(&name);
                this.model_info.insert(name, info);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn load_model(&mut self, name: String, config: LoadConfig, cx: &mut Context<Self>) {
        if let Some(g) = &self.generation {
            g.cancel.store(true, Ordering::Relaxed);
        }
        self.settings.last_model = Some(name.clone());
        self.settings.model_load.insert(name.clone(), config.clone());
        self.save_settings();
        self.model_state = ModelState::Loading {
            name: name.clone(),
            since: Instant::now(),
        };
        self.engine.send(Command::Load { name, config });
        cx.notify();
    }

    pub fn eject_model(&mut self, cx: &mut Context<Self>) {
        if let Some(g) = &self.generation {
            g.cancel.store(true, Ordering::Relaxed);
        }
        self.engine.send(Command::Unload);
        cx.notify();
    }

    pub fn delete_model(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.model_state.loaded().is_some_and(|m| m.name == name) {
            self.eject_model(cx);
        }
        let n = name.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = crate::runtime::spawn_blocking(move || catalog::remove(&n))
                .await
                .and_then(|r| r);
            let _ = this.update_in(cx, |this, window, cx| {
                match res {
                    Ok(()) => {
                        this.notify_success(format!("Deleted {name}"), window, cx);
                        if this.selected_model.as_deref() == Some(&name) {
                            this.selected_model = None;
                        }
                        this.model_info.remove(&name);
                    }
                    Err(e) => this.notify_error(format!("Cannot delete {name}: {e:#}"), window, cx),
                }
                this.refresh_models(cx);
            });
        })
        .detach();
    }

    fn on_engine_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Loading { name } => {
                self.model_state = ModelState::Loading {
                    name,
                    since: Instant::now(),
                };
            }
            Event::Loaded(info) => {
                self.notify_success(
                    format!("{} loaded in {:.1}s", info.name, info.load_ms as f32 / 1000.0),
                    window,
                    cx,
                );
                self.model_state = ModelState::Loaded(info);
            }
            Event::LoadFailed { name, error } => {
                self.model_state = ModelState::Unloaded;
                self.notify_error_with_logs(format!("Failed to load {name}: {error}"), window, cx);
            }
            Event::Unloaded => {
                // a load in progress replaced the model, keep showing it
                if !matches!(self.model_state, ModelState::Loading { .. }) {
                    self.model_state = ModelState::Unloaded;
                }
            }
            Event::Started { id, prompt_tokens } => {
                if let Some(g) = self.generation.as_mut().filter(|g| g.id == id) {
                    g.prompt_done = Some(Instant::now());
                    g.prompt_tokens = prompt_tokens;
                }
            }
            Event::Token { id, text } => {
                let Some(g) = self.generation.as_mut().filter(|g| g.id == id) else {
                    return;
                };
                g.chunks += 1;
                let (chat_id, message_id, started) = (g.chat.clone(), g.message.clone(), g.started);
                if let Some(message) = self
                    .chat_mut(&chat_id)
                    .and_then(|c| c.messages.iter_mut().find(|m| m.id == message_id))
                {
                    let was_done = conversation::split_reasoning(&message.content).reasoning_done;
                    message.content.push_str(&text);
                    let reply = conversation::split_reasoning(&message.content);
                    if reply.reasoning.is_some() && reply.reasoning_done && !was_done {
                        message.reasoning_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    let content = message.content.clone();
                    if let Some(views) = self.views.get_mut(&message_id) {
                        views.sync(Role::Assistant, &content, cx);
                    }
                }
                self.remeasure_message(&chat_id, &message_id, cx);
            }
            Event::Finished { id, text, stats } => {
                let Some(g) = self.generation.take_if(|g| g.id == id) else {
                    return;
                };
                if let Some(chat) = self.chat_mut(&g.chat) {
                    if let Some(message) = chat.messages.iter_mut().find(|m| m.id == g.message) {
                        message.content = text;
                        message.stats = Some(stats);
                    }
                    chat.updated = Utc::now();
                }
                self.sync_message_views(&g.chat, &g.message, cx);
                self.remeasure_message(&g.chat, &g.message, cx);
                self.save_chat(&g.chat);
            }
            Event::Failed { id, error } => {
                let Some(g) = self.generation.take_if(|g| g.id == id) else {
                    return;
                };
                if let Some(chat) = self.chat_mut(&g.chat) {
                    if let Some(message) = chat.messages.iter_mut().find(|m| m.id == g.message) {
                        message.error = Some(error.clone());
                    }
                }
                self.remeasure_message(&g.chat, &g.message, cx);
                self.save_chat(&g.chat);
                self.notify_error(error, window, cx);
            }
        }
    }

    // ---------------------------------------------------------------- chats

    pub fn new_chat(&mut self, project: Option<Id>, window: &mut Window, cx: &mut Context<Self>) {
        // reuse an empty chat of the same project rather than piling them up
        if let Some(existing) = self
            .chats
            .iter()
            .find(|c| c.messages.is_empty() && c.project == project)
            .map(|c| c.id.clone())
        {
            self.select_chat(existing, window, cx);
            return;
        }
        let chat = Chat::new(
            project,
            self.settings.system_prompt.clone(),
            self.settings.sampling.clone(),
        );
        let id = chat.id.clone();
        self.store.save_chat(&chat);
        self.chats.insert(0, chat);
        self.select_chat(id, window, cx);
    }

    pub fn select_chat(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat(&id) else {
            return;
        };
        let system_prompt = chat.system_prompt.clone();
        let sampling = chat.sampling.clone();
        let project = chat.project.clone();
        self.active_chat = Some(id);
        self.page = Page::Chat;
        self.editing = None;
        self.pending_attachments.clear();
        if let RightPanel::Project(_) = self.right_panel {
            self.right_panel = RightPanel::Settings;
        }

        self.system_prompt
            .update(cx, |s, cx| s.set_value(system_prompt, window, cx));
        self.max_tokens.update(cx, |s, cx| {
            s.set_value(
                sampling.max_tokens.map(|v| v.to_string()).unwrap_or_default(),
                window,
                cx,
            )
        });
        self.seed.update(cx, |s, cx| {
            s.set_value(
                sampling.seed.map(|v| v.to_string()).unwrap_or_default(),
                window,
                cx,
            )
        });
        self.set_sampling_sliders(&sampling, window, cx);
        self.sync_rows(cx);

        // nothing loaded: load the project's model
        if matches!(self.model_state, ModelState::Unloaded) {
            if let Some(model) = project
                .and_then(|p| self.project(&p))
                .and_then(|p| p.default_model.clone())
            {
                let config = self.load_config_for(&model);
                self.load_model(model, config, cx);
            }
        }
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    fn set_sampling_sliders(&self, s: &Sampling, window: &mut Window, cx: &mut Context<Self>) {
        let sliders = &self.sliders;
        for (slider, value) in [
            (&sliders.temperature, s.temperature),
            (&sliders.top_k, s.top_k as f32),
            (&sliders.top_p, s.top_p),
            (&sliders.min_p, s.min_p),
            (&sliders.repeat_penalty, s.repeat_penalty),
        ] {
            slider.update(cx, |sl, cx| sl.set_value(value, window, cx));
        }
    }

    pub fn reset_sampling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sampling = Sampling::default();
        self.with_active_chat(cx, true, |chat| chat.sampling = sampling.clone());
        self.set_sampling_sliders(&sampling, window, cx);
        self.max_tokens.update(cx, |s, cx| s.set_value("", window, cx));
        self.seed.update(cx, |s, cx| s.set_value("", window, cx));
    }

    pub fn save_sampling_as_default(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((sampling, system_prompt)) = self
            .active()
            .map(|c| (c.sampling.clone(), c.system_prompt.clone()))
        {
            self.settings.sampling = sampling;
            self.settings.system_prompt = system_prompt;
            self.save_settings();
            self.notify_success("Saved as the default for new chats", window, cx);
        }
    }

    pub fn delete_chat(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        if self.generation.as_ref().is_some_and(|g| g.chat == id) {
            self.stop_generation(cx);
        }
        self.chats.retain(|c| c.id != id);
        self.store.delete_chat(&id);
        if self.active_chat.as_deref() == Some(&id) {
            self.active_chat = None;
            match self.chats.first().map(|c| c.id.clone()) {
                Some(next) => self.select_chat(next, window, cx),
                None => self.new_chat(None, window, cx),
            }
        }
        cx.notify();
    }

    pub fn rename_chat(&mut self, id: &str, title: String, cx: &mut Context<Self>) {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        if let Some(chat) = self.chat_mut(id) {
            chat.title = title;
            chat.title_locked = true;
        }
        self.save_chat(id);
        cx.notify();
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat_mut(id) {
            chat.pinned = !chat.pinned;
        }
        self.save_chat(id);
        cx.notify();
    }

    pub fn move_chat(&mut self, id: &str, project: Option<Id>, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat_mut(id) {
            chat.project = project;
        }
        self.save_chat(id);
        cx.notify();
    }

    pub fn duplicate_chat(&mut self, id: &str, upto: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat(id) else {
            return;
        };
        let mut copy = chat.clone();
        copy.id = crate::store::new_id();
        copy.title = format!("{} (branch)", chat.title);
        copy.created = Utc::now();
        copy.updated = Utc::now();
        copy.pinned = false;
        if let Some(upto) = upto {
            if let Some(ix) = copy.messages.iter().position(|m| m.id == upto) {
                copy.messages.truncate(ix + 1);
            }
        }
        for m in &mut copy.messages {
            m.id = crate::store::new_id();
        }
        let new_id = copy.id.clone();
        self.store.save_chat(&copy);
        self.chats.insert(0, copy);
        self.select_chat(new_id, window, cx);
    }

    /// Copy the whole conversation as Markdown (the text of the export)
    pub fn copy_chat(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat(id) {
            let markdown = conversation::to_markdown(chat);
            self.copy(markdown, window, cx);
        }
    }

    pub fn export_chat(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat(id) else {
            return;
        };
        let markdown = conversation::to_markdown(chat);
        let name: String = chat
            .title
            .chars()
            .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
            .collect();
        let dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{}.md", name.trim())));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let res = std::fs::write(&path, markdown);
            let _ = this.update_in(cx, |this, window, cx| match res {
                Ok(()) => this.notify_success(format!("Exported to {}", path.display()), window, cx),
                Err(e) => this.notify_error(format!("Export failed: {e}"), window, cx),
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------- projects

    pub fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.projects.len() + 1;
        let project = Project::new(format!("Project {n}"));
        let id = project.id.clone();
        self.projects.push(project);
        self.save_projects();
        self.open_project_settings(id, window, cx);
    }

    pub fn open_project_settings(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project(&id) else {
            return;
        };
        let (name, instructions) = (project.name.clone(), project.instructions.clone());
        self.project_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.project_instructions
            .update(cx, |s, cx| s.set_value(instructions, window, cx));
        self.right_panel = RightPanel::Project(id);
        self.settings.show_right_panel = true;
        self.page = Page::Chat;
        cx.notify();
    }

    pub fn delete_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.projects.retain(|p| p.id != id);
        // its chats are kept, outside any project
        let ids: Vec<Id> = self
            .chats
            .iter_mut()
            .filter(|c| c.project.as_deref() == Some(id))
            .map(|c| {
                c.project = None;
                c.id.clone()
            })
            .collect();
        for chat in ids {
            self.save_chat(&chat);
        }
        if self.right_panel == RightPanel::Project(id.to_string()) {
            self.right_panel = RightPanel::Settings;
        }
        self.save_projects();
        cx.notify();
    }

    pub fn toggle_project(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
            p.expanded = !p.expanded;
        }
        self.save_projects();
        cx.notify();
    }

    pub fn add_project_files(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add to project".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let (files, errors) = read_text_files(paths);
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                    for (name, path, content) in files {
                        p.files.retain(|f| f.path.as_ref() != Some(&path));
                        p.files.push(ProjectFile {
                            name,
                            path: Some(path),
                            content,
                        });
                    }
                }
                this.save_projects();
                for e in errors {
                    this.notify_error(e, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn attach_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let (files, errors) = read_text_files(paths);
            let _ = this.update_in(cx, |this, window, cx| {
                for (name, _, content) in files {
                    this.pending_attachments.push(Attachment { name, content });
                }
                for e in errors {
                    this.notify_error(e, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------- messages

    pub fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_generating() {
            return;
        }
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() && self.pending_attachments.is_empty() {
            return;
        }
        if self.model_state.loaded().is_none() {
            let msg = match &self.model_state {
                ModelState::Loading { .. } => "The model is still loading",
                _ => "Load a model to chat",
            };
            self.notify_info(msg, window, cx);
            if matches!(self.model_state, ModelState::Unloaded) {
                self.open_picker(window, cx);
            }
            return;
        }
        let Some(chat_id) = self.active_chat.clone() else {
            return;
        };
        let attachments = std::mem::take(&mut self.pending_attachments);
        if let Some(chat) = self.chat_mut(&chat_id) {
            let mut message = ChatMessage::new(Role::User, text.clone());
            message.attachments = attachments;
            chat.messages.push(message);
            if !chat.title_locked && chat.messages.len() == 1 {
                chat.title = conversation::title_from(&text);
            }
            chat.updated = Utc::now();
        }
        // most recently used first
        if let Some(ix) = self.chats.iter().position(|c| c.id == chat_id) {
            let chat = self.chats.remove(ix);
            self.chats.insert(0, chat);
        }
        self.composer.update(cx, |c, cx| c.set_value("", window, cx));
        self.start_generation(chat_id, window, cx);
    }

    /// Generate the next assistant message of a chat, from all its messages
    fn start_generation(&mut self, chat_id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let Some(loaded) = self.model_state.loaded().cloned() else {
            self.notify_info("Load a model to chat", window, cx);
            return;
        };
        let Some(chat) = self.chat(&chat_id) else {
            return;
        };
        let messages = conversation::transcript(chat, self.project_of(chat), &chat.messages);
        let sampling = chat.sampling.clone();

        let mut reply = ChatMessage::new(Role::Assistant, "");
        reply.model = Some(loaded.name.clone());
        let message_id = reply.id.clone();
        if let Some(chat) = self.chat_mut(&chat_id) {
            chat.messages.push(reply);
        }
        self.save_chat(&chat_id);

        let id = self.next_request;
        self.next_request += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.generation = Some(Generation {
            id,
            chat: chat_id,
            message: message_id,
            cancel: cancel.clone(),
            started: Instant::now(),
            prompt_done: None,
            prompt_tokens: 0,
            chunks: 0,
        });
        self.engine.send(Command::Generate(GenerateRequest {
            id,
            messages,
            sampling,
            cancel,
        }));
        self.sync_rows(cx);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        cx.notify();
    }

    pub fn stop_generation(&mut self, cx: &mut Context<Self>) {
        if let Some(g) = &self.generation {
            g.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Regenerate an assistant message (and drop what follows it)
    pub fn regenerate(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_generating() {
            return;
        }
        let Some(chat_id) = self.active_chat.clone() else {
            return;
        };
        if let Some(chat) = self.chat_mut(&chat_id) {
            if let Some(ix) = chat.messages.iter().position(|m| m.id == message_id) {
                chat.messages.truncate(ix);
            }
        }
        self.views.remove(message_id);
        self.reset_rows(cx);
        self.start_generation(chat_id, window, cx);
    }

    pub fn delete_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if self.generation.as_ref().is_some_and(|g| g.message == message_id) {
            self.stop_generation(cx);
        }
        let Some(chat_id) = self.active_chat.clone() else {
            return;
        };
        if let Some(chat) = self.chat_mut(&chat_id) {
            chat.messages.retain(|m| m.id != message_id);
        }
        self.views.remove(message_id);
        self.save_chat(&chat_id);
        self.reset_rows(cx);
        cx.notify();
    }

    pub fn start_edit(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self
            .active()
            .and_then(|c| c.messages.iter().find(|m| m.id == message_id))
            .map(|m| m.content.clone())
        else {
            return;
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 16)
                .default_value(content)
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.editing = Some((message_id.to_string(), input));
        self.reset_rows(cx);
        cx.notify();
    }

    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        self.reset_rows(cx);
        cx.notify();
    }

    /// Save an edited message; for a user message, `resubmit` regenerates the
    /// reply from it
    pub fn commit_edit(&mut self, resubmit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((message_id, input)) = self.editing.take() else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let Some(chat_id) = self.active_chat.clone() else {
            return;
        };
        let mut is_user = false;
        if let Some(chat) = self.chat_mut(&chat_id) {
            if let Some(ix) = chat.messages.iter().position(|m| m.id == message_id) {
                chat.messages[ix].content = text;
                is_user = chat.messages[ix].role == Role::User;
                if resubmit && is_user {
                    chat.messages.truncate(ix + 1);
                }
            }
        }
        self.sync_message_views(&chat_id, &message_id, cx);
        self.save_chat(&chat_id);
        self.reset_rows(cx);
        if resubmit && is_user && !self.is_generating() {
            self.start_generation(chat_id, window, cx);
        }
        cx.notify();
    }

    pub fn toggle_reasoning(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if !self.expanded_reasoning.remove(message_id) {
            self.expanded_reasoning.insert(message_id.to_string());
        }
        if let Some(chat_id) = self.active_chat.clone() {
            self.remeasure_message(&chat_id, message_id, cx);
        }
        cx.notify();
    }

    /// The markdown views of a message, created on first use
    pub fn message_views(&mut self, message: &ChatMessage, cx: &mut App) -> (Entity<TextViewState>, Entity<TextViewState>) {
        let views = self
            .views
            .entry(message.id.clone())
            .or_insert_with(|| MessageViews::new(cx));
        views.sync(message.role, &message.content, cx);
        (views.answer.clone(), views.reasoning.clone())
    }

    fn sync_message_views(&mut self, chat_id: &str, message_id: &str, cx: &mut App) {
        let Some(message) = self
            .chat(chat_id)
            .and_then(|c| c.messages.iter().find(|m| m.id == message_id))
            .map(|m| (m.role, m.content.clone()))
        else {
            return;
        };
        if let Some(views) = self.views.get_mut(message_id) {
            views.sync(message.0, &message.1, cx);
        }
    }

    // ---------------------------------------------------------------- message list rows

    /// Keep the virtual list's row count in step with the active chat
    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let count = self.active().map(|c| c.messages.len()).unwrap_or(0);
        let same_chat = self.scroller_chat == self.active_chat;
        self.scroller_chat = self.active_chat.clone();
        self.scroller.update(cx, |s, cx| {
            let current = s.item_count();
            if same_chat && count > current {
                s.append(count - current, cx);
            } else if !same_chat || count != current {
                s.reset(count, cx);
            }
        });
    }

    fn reset_rows(&mut self, cx: &mut Context<Self>) {
        let count = self.active().map(|c| c.messages.len()).unwrap_or(0);
        self.scroller_chat = self.active_chat.clone();
        self.scroller.update(cx, |s, cx| {
            if s.item_count() == count {
                s.remeasure(cx);
            } else {
                s.reset(count, cx);
            }
        });
    }

    fn remeasure_message(&mut self, chat_id: &str, message_id: &str, cx: &mut Context<Self>) {
        if self.active_chat.as_deref() != Some(chat_id) {
            return;
        }
        let Some(ix) = self
            .chat(chat_id)
            .and_then(|c| c.messages.iter().position(|m| m.id == message_id))
        else {
            return;
        };
        self.scroller.update(cx, |s, cx| {
            if !s.remeasure_items(ix..ix + 1, cx) {
                s.remeasure(cx);
            }
        });
    }

    // ---------------------------------------------------------------- downloads

    pub fn start_download(
        &mut self,
        name: String,
        expected: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .downloads
            .iter()
            .any(|d| d.name == name && matches!(d.status, DownloadStatus::Running))
        {
            self.notify_info(format!("{name} is already downloading"), window, cx);
            return;
        }
        let id = self.next_download;
        self.next_download += 1;
        let progress = DownloadProgress::new(expected);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = {
            let (name, progress) = (name.clone(), progress.clone());
            crate::runtime::rt().spawn(async move {
                let _ = tx.send(crate::hub::pull(name, progress).await);
            })
        };
        logs::info("download", format!("pulling {name}"));
        self.downloads.insert(
            0,
            Download {
                id,
                name: name.clone(),
                progress,
                status: DownloadStatus::Running,
                started: Instant::now(),
                abort: Some(task.abort_handle()),
            },
        );
        self.downloads_open = true;
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let status = match result {
                    Ok(Ok(installed)) => {
                        logs::info("download", format!("{installed} downloaded"));
                        this.notify_success(format!("{installed} downloaded"), window, cx);
                        DownloadStatus::Done(installed)
                    }
                    Ok(Err(e)) => {
                        let e = format!("{e:#}");
                        logs::error("download", format!("{name}: {e}"));
                        this.notify_error(format!("Download of {name} failed"), window, cx);
                        DownloadStatus::Failed(e)
                    }
                    Err(_) => DownloadStatus::Cancelled,
                };
                let Some(d) = this.downloads.iter_mut().find(|d| d.id == id) else {
                    return;
                };
                d.abort = None;
                d.status = status;
                this.refresh_models(cx);
                cx.notify();
            });
        })
        .detach();
        self.ensure_ticker(cx);
        cx.notify();
    }

    pub fn cancel_download(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(d) = self.downloads.iter_mut().find(|d| d.id == id) {
            if let Some(abort) = d.abort.take() {
                abort.abort();
            }
            d.status = DownloadStatus::Cancelled;
        }
        cx.notify();
    }

    pub fn clear_finished_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads
            .retain(|d| matches!(d.status, DownloadStatus::Running));
        cx.notify();
    }

    pub fn running_downloads(&self) -> usize {
        self.downloads
            .iter()
            .filter(|d| matches!(d.status, DownloadStatus::Running))
            .count()
    }

    /// Redraw periodically while something progresses without events
    fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        let keep = this.running_downloads() > 0
                            || matches!(this.model_state, ModelState::Loading { .. });
                        if !keep {
                            this.ticker = None;
                        }
                        keep
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    // ---------------------------------------------------------------- server

    pub fn toggle_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(server) = self.server.take() {
            server.stop();
            cx.notify();
            return;
        }
        if self.server_busy {
            return;
        }
        let host = self.server_host.read(cx).value().trim().to_string();
        let port = match self.server_port.read(cx).value().trim().parse::<u16>() {
            Ok(p) => p,
            Err(_) => {
                self.notify_error("The port must be a number between 1 and 65535", window, cx);
                return;
            }
        };
        self.settings.server_host = host.clone();
        self.settings.server_port = port;
        self.save_settings();
        self.server_busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let result = crate::server::start(host, port).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.server_busy = false;
                match result {
                    Ok(handle) => this.server = Some(handle),
                    Err(e) => this.notify_error(format!("Cannot start the server: {e:#}"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn load_config_for(&self, model: &str) -> LoadConfig {
        self.settings
            .model_load
            .get(model)
            .cloned()
            .unwrap_or_else(|| self.settings.load.clone())
    }
}

/// Read text files for attachments, with the reasons the others were skipped
fn read_text_files(paths: Vec<PathBuf>) -> (Vec<(String, PathBuf, String)>, Vec<String>) {
    const MAX: u64 = 2 * 1024 * 1024;
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX => {
                errors.push(format!("{name} is too large to attach (over 2 MB)"));
                continue;
            }
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                continue;
            }
            _ => {}
        }
        match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => files.push((name, path, text)),
                Err(_) => errors.push(format!("{name} is not a text file")),
            },
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (files, errors)
}

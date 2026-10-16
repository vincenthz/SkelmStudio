//! The persisted state: settings, projects and chats, as JSON files under the
//! platform data directory (`~/Library/Application Support/skelm-studio` on
//! macOS, `$XDG_DATA_HOME/skelm-studio` on Linux).
//!
//! Writes go through a single writer thread so they never block the UI and
//! two saves of the same file always land in order.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Id = String;

pub fn new_id() -> Id {
    ulid::Ulid::new().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// A text file attached to a user message, inlined into the prompt
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// the model produced its end-of-generation token
    Eos,
    /// the response length limit was reached
    MaxTokens,
    /// the context window is full
    ContextFull,
    /// stopped by the user
    Cancelled,
    Error,
}

impl StopReason {
    pub fn label(self) -> &'static str {
        match self {
            StopReason::Eos => "EOS token found",
            StopReason::MaxTokens => "Max tokens reached",
            StopReason::ContextFull => "Context full",
            StopReason::Cancelled => "User stopped",
            StopReason::Error => "Error",
        }
    }
}

/// Measurements of one generation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenStats {
    pub prompt_tokens: u32,
    pub tokens: u32,
    pub tokens_per_sec: f32,
    pub time_to_first_token_ms: u64,
    pub total_ms: u64,
    pub stop_reason: StopReason,
    /// the context window size the model was loaded with
    #[serde(default)]
    pub context_size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Id,
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// the model that produced an assistant message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<GenStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// how long the model reasoned before answering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_ms: Option<u64>,
    pub created: DateTime<Utc>,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            role,
            content: content.into(),
            attachments: Vec::new(),
            model: None,
            stats: None,
            error: None,
            reasoning_ms: None,
            created: Utc::now(),
        }
    }
}

/// Sampling parameters of a chat
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sampling {
    pub temperature: f32,
    /// 0 disables top-k
    pub top_k: u32,
    /// 1.0 disables top-p
    pub top_p: f32,
    /// 0.0 disables min-p
    pub min_p: f32,
    /// 1.0 disables the repeat penalty
    pub repeat_penalty: f32,
    pub repeat_last_n: u32,
    /// limit of the response length, in tokens
    pub max_tokens: Option<u32>,
    /// fixed seed, random when `None`
    pub seed: Option<u32>,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            temperature: 0.8,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
            max_tokens: None,
            seed: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: Id,
    pub title: String,
    /// whether the title was set by the user, so it isn't auto-generated
    #[serde(default)]
    pub title_locked: bool,
    #[serde(default)]
    pub project: Option<Id>,
    #[serde(default)]
    pub pinned: bool,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub sampling: Sampling,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

impl Chat {
    pub fn new(project: Option<Id>, system_prompt: String, sampling: Sampling) -> Self {
        let now = Utc::now();
        Self {
            id: new_id(),
            title: "New Chat".into(),
            title_locked: false,
            project,
            pinned: false,
            created: now,
            updated: now,
            system_prompt,
            sampling,
            messages: Vec::new(),
        }
    }
}

/// A reference document of a project, given to every chat of the project
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub name: String,
    /// instructions prepended to the system prompt of every chat of the project
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub files: Vec<ProjectFile>,
    /// the model loaded when a chat of the project is opened with nothing loaded
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default = "default_true")]
    pub expanded: bool,
    pub created: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            instructions: String::new(),
            files: Vec::new(),
            default_model: None,
            expanded: true,
            created: Utc::now(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashAttention {
    #[default]
    Auto,
    On,
    Off,
}

/// How a model is loaded
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoadConfig {
    /// context window, in tokens
    pub context_length: u32,
    /// layers offloaded to the GPU, negative for all of them
    pub gpu_layers: i32,
    pub flash_attention: FlashAttention,
    /// memory-map the weights
    pub mmap: bool,
    /// speculative decoding with the model's MTP layers, when it has some
    pub mtp: bool,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            context_length: 8192,
            gpu_layers: -1,
            flash_attention: FlashAttention::Auto,
            mmap: true,
            mtp: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dark_mode: bool,
    pub load: LoadConfig,
    pub sampling: Sampling,
    pub system_prompt: String,
    pub server_host: String,
    pub server_port: u16,
    pub show_right_panel: bool,
    pub show_chat_list: bool,
    /// the last model loaded, offered first in the picker
    pub last_model: Option<String>,
    /// per-model load settings, remembered from the last load
    pub model_load: std::collections::BTreeMap<String, LoadConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            load: LoadConfig::default(),
            sampling: Sampling::default(),
            system_prompt: String::new(),
            server_host: "127.0.0.1".into(),
            server_port: 1234,
            show_right_panel: true,
            show_chat_list: true,
            last_model: None,
            model_load: Default::default(),
        }
    }
}

enum WriteOp {
    Write(PathBuf, String),
    Remove(PathBuf),
}

/// The on-disk store
pub struct Store {
    root: PathBuf,
    writer: mpsc::Sender<WriteOp>,
}

impl Store {
    pub fn open() -> anyhow::Result<Self> {
        let root = std::env::var_os("SKELM_STUDIO_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|d| d.join("skelm-studio")))
            .ok_or_else(|| anyhow::anyhow!("cannot find a data directory"))?;
        std::fs::create_dir_all(root.join("chats"))?;

        let (writer, rx) = mpsc::channel::<WriteOp>();
        std::thread::Builder::new()
            .name("skelm-store".into())
            .spawn(move || {
                for op in rx {
                    let res = match &op {
                        WriteOp::Write(path, data) => write_atomic(path, data),
                        WriteOp::Remove(path) => match std::fs::remove_file(path) {
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            r => r,
                        },
                    };
                    if let Err(e) = res {
                        let path = match &op {
                            WriteOp::Write(p, _) | WriteOp::Remove(p) => p,
                        };
                        eprintln!("store: {}: {e}", path.display());
                    }
                }
            })?;
        Ok(Self { root, writer })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, path: &Path) -> Option<T> {
        let data = std::fs::read_to_string(path).ok()?;
        match serde_json::from_str(&data) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("store: ignoring {}: {e}", path.display());
                None
            }
        }
    }

    fn write<T: Serialize>(&self, path: PathBuf, value: &T) {
        match serde_json::to_string_pretty(value) {
            Ok(data) => {
                let _ = self.writer.send(WriteOp::Write(path, data));
            }
            Err(e) => eprintln!("store: cannot serialize {}: {e}", path.display()),
        }
    }

    pub fn load_settings(&self) -> Settings {
        self.read(&self.root.join("settings.json"))
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        self.write(self.root.join("settings.json"), settings)
    }

    pub fn load_projects(&self) -> Vec<Project> {
        self.read(&self.root.join("projects.json"))
            .unwrap_or_default()
    }

    pub fn save_projects(&self, projects: &[Project]) {
        self.write(self.root.join("projects.json"), &projects)
    }

    fn chat_path(&self, id: &str) -> PathBuf {
        self.root.join("chats").join(format!("{id}.json"))
    }

    /// Every chat, most recently updated first
    pub fn load_chats(&self) -> Vec<Chat> {
        let mut chats: Vec<Chat> = std::fs::read_dir(self.root.join("chats"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| self.read(&e.path()))
            .collect();
        chats.sort_by(|a, b| b.updated.cmp(&a.updated));
        chats
    }

    pub fn save_chat(&self, chat: &Chat) {
        self.write(self.chat_path(&chat.id), chat)
    }

    pub fn delete_chat(&self, id: &str) {
        let _ = self.writer.send(WriteOp::Remove(self.chat_path(id)));
    }
}

fn write_atomic(path: &Path, data: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

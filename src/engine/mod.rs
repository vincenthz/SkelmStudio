//! The inference engine: a dedicated thread owning the loaded model.
//!
//! The UI sends [`Command`]s and receives [`Event`]s on an async channel. A
//! generation runs to completion on the engine thread (llama.cpp already uses
//! every core), the UI stops one early with its cancel flag. Each generation
//! renders the whole transcript through the model's chat template into a fresh
//! context, like `skelm run` and `skelm serve` do.

pub mod sampler;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use skelm_exec::{
    ContextOptions, ContextParams, DraftParams, Drafter, FlashAttn, LoadMode, Message, Model,
    ModelDescr, ModelParameters, ModelParams, SpeculativeConfig,
};

use crate::logs;
use crate::store::{FlashAttention, GenStats, LoadConfig, Sampling, StopReason};

/// What is known about the loaded model
#[derive(Clone, Debug)]
pub struct LoadedModel {
    pub name: String,
    pub config: LoadConfig,
    /// the context window the contexts are created with
    pub context_length: u32,
    /// the context window the model was trained with
    pub train_context_length: u32,
    pub architecture: Option<String>,
    pub description: String,
    pub has_mtp: bool,
    pub speculative: bool,
    pub load_ms: u64,
}

pub struct GenerateRequest {
    pub id: u64,
    pub messages: Vec<Message>,
    pub sampling: Sampling,
    pub cancel: Arc<AtomicBool>,
}

pub enum Command {
    Load { name: String, config: LoadConfig },
    Unload,
    Generate(GenerateRequest),
}

pub enum Event {
    Loading { name: String },
    Loaded(LoadedModel),
    LoadFailed { name: String, error: String },
    Unloaded,
    /// the prompt was processed, generation starts
    Started { id: u64, prompt_tokens: u32 },
    Token { id: u64, text: String },
    Finished { id: u64, text: String, stats: GenStats },
    Failed { id: u64, error: String },
}

#[derive(Clone)]
pub struct Engine {
    commands: mpsc::Sender<Command>,
}

impl Engine {
    pub fn spawn() -> (Self, async_channel::Receiver<Event>) {
        let (commands, rx) = mpsc::channel();
        let (events, events_rx) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("skelm-engine".into())
            // llama.cpp and the template engine recurse, give them room
            .stack_size(16 * 1024 * 1024)
            .spawn(move || EngineThread::default().run(rx, events))
            .expect("failed to start the engine thread");
        (Self { commands }, events_rx)
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

#[derive(Default)]
struct EngineThread {
    model: Option<(Model, LoadedModel)>,
}

impl EngineThread {
    fn run(mut self, commands: mpsc::Receiver<Command>, events: async_channel::Sender<Event>) {
        let emit = |e: Event| {
            let _ = events.send_blocking(e);
        };
        for command in commands {
            match command {
                Command::Load { name, config } => {
                    // drop the previous model first, both rarely fit together
                    if self.model.take().is_some() {
                        emit(Event::Unloaded);
                    }
                    emit(Event::Loading { name: name.clone() });
                    match with_llama_reason(|| load(&name, &config)) {
                        Ok((model, info)) => {
                            emit(Event::Loaded(info.clone()));
                            self.model = Some((model, info));
                        }
                        Err(error) => {
                            logs::error("studio", format!("failed to load {name}: {error}"));
                            emit(Event::LoadFailed { name, error })
                        }
                    }
                }
                Command::Unload => {
                    if self.model.take().is_some() {
                        emit(Event::Unloaded);
                    }
                }
                Command::Generate(req) => {
                    let id = req.id;
                    let Some((model, info)) = &self.model else {
                        emit(Event::Failed {
                            id,
                            error: "no model is loaded".into(),
                        });
                        continue;
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        with_llama_reason(|| generate(model, info, req, &emit))
                    }));
                    match result {
                        Ok(Ok((text, stats))) => emit(Event::Finished { id, text, stats }),
                        Ok(Err(error)) => {
                            logs::error("studio", format!("generation failed: {error}"));
                            emit(Event::Failed { id, error })
                        }
                        Err(_) => emit(Event::Failed {
                            id,
                            error: "the generation panicked".into(),
                        }),
                    }
                }
            }
        }
    }
}

/// Run `f`, describing its error with the reasons llama.cpp logged (rather
/// than skelm-core's "consult logging for reasons")
fn with_llama_reason<T>(f: impl FnOnce() -> anyhow::Result<T>) -> Result<T, String> {
    let (result, errors) = logs::capture_llama_errors(f);
    result.map_err(|e| {
        let error = format!("{e:#}");
        match logs::llama_reason(&errors) {
            Some(reason) if error.contains("consult logging") => reason,
            Some(reason) => format!("{error}: {reason}"),
            None => error,
        }
    })
}

fn load(name: &str, config: &LoadConfig) -> anyhow::Result<(Model, LoadedModel)> {
    let start = Instant::now();
    let descr = ModelDescr::parse(name).map_err(anyhow::Error::msg)?;
    let params = ModelParams {
        n_gpu_layers: config.gpu_layers,
        load_mode: if config.mmap {
            LoadMode::Mmap
        } else {
            LoadMode::None
        },
        load_mtp: config.mtp,
        ..ModelParams::default()
    };
    let model = Model::load_with_params(&descr, &params)?;

    let train_context_length = model.model.n_ctx_train().max(0) as u32;
    let context_length = match config.context_length {
        0 => train_context_length,
        n => n,
    };
    let context = ContextParams {
        n_ctx: context_length,
        flash_attn: match config.flash_attention {
            FlashAttention::Auto => FlashAttn::Auto,
            FlashAttention::On => FlashAttn::Enabled,
            FlashAttention::Off => FlashAttn::Disabled,
        },
        ..ContextParams::default()
    };
    let has_mtp = model.model.has_mtp();
    let speculative = (config.mtp && has_mtp).then(|| SpeculativeConfig {
        drafter: Drafter::Mtp,
        draft: DraftParams::default(),
    });
    let speculative_on = speculative.is_some();
    let model = model.with_context_options(ContextOptions {
        params: context,
        lora: Vec::new(),
        speculative,
    })?;
    // fail now rather than on the first message if the context doesn't fit
    drop(model.try_new_context()?);

    let info = LoadedModel {
        name: name.to_string(),
        config: config.clone(),
        context_length,
        train_context_length,
        architecture: model.model.architecture(),
        description: model.model.description().trim_end_matches('\0').to_string(),
        has_mtp,
        speculative: speculative_on,
        load_ms: start.elapsed().as_millis() as u64,
    };
    Ok((model, info))
}

/// Decode as much of `pending` as forms valid UTF-8, keeping an incomplete
/// trailing sequence for the next token
fn drain_utf8(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&pending[..valid]).unwrap());
                match e.error_len() {
                    Some(bad) => {
                        out.push(char::REPLACEMENT_CHARACTER);
                        pending.drain(..valid + bad);
                    }
                    None => {
                        pending.drain(..valid);
                        return out;
                    }
                }
            }
        }
    }
}

fn generate(
    model: &Model,
    info: &LoadedModel,
    req: GenerateRequest,
    emit: &impl Fn(Event),
) -> anyhow::Result<(String, GenStats)> {
    let start = Instant::now();
    let id = req.id;
    let cancel = req.cancel;
    let prompt = model.model_template_render(&ModelParameters::new(req.messages));

    let vocab = model.vocab.clone();
    let tokens = vocab.tokenize(prompt.as_bytes(), true);
    let n_ctx = info.context_length as usize;
    if tokens.len() >= n_ctx {
        anyhow::bail!(
            "the conversation ({} tokens) doesn't fit in the context window ({n_ctx} tokens): \
             reload the model with a longer context, or start a new chat",
            tokens.len()
        );
    }

    let mut context = model.try_new_context()?;
    let ctx = &mut context.1;

    // process the prompt in chunks so a long one can be cancelled
    for chunk in tokens.chunks(256) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        ctx.append_tokens(chunk)?;
    }
    let prompt_tokens = tokens.len() as u32;
    emit(Event::Started { id, prompt_tokens });

    let mut sampler = sampler::build(&req.sampling);
    let max_tokens = req.sampling.max_tokens.map(|n| n as usize);

    let mut text = String::new();
    let mut pending = Vec::new();
    let mut generated = 0usize;
    let mut first_token: Option<Instant> = None;
    let stop_reason = loop {
        if cancel.load(Ordering::Relaxed) {
            break StopReason::Cancelled;
        }
        if max_tokens.is_some_and(|max| generated >= max) {
            break StopReason::MaxTokens;
        }
        if ctx.n_past() + 1 >= n_ctx {
            break StopReason::ContextFull;
        }
        let step = ctx.step(&mut sampler, &vocab)?;
        if first_token.is_none() {
            first_token = Some(Instant::now());
        }
        generated += step.tokens.len();
        for token in step.tokens {
            if vocab.token_attr(token).is_control() {
                continue;
            }
            pending.extend_from_slice(&vocab.as_bytes(token));
        }
        let chunk = drain_utf8(&mut pending);
        if !chunk.is_empty() {
            text.push_str(&chunk);
            emit(Event::Token { id, text: chunk });
        }
        if step.eog {
            break StopReason::Eos;
        }
    };
    if !pending.is_empty() {
        text.push_str(&String::from_utf8_lossy(&pending));
    }

    let end = Instant::now();
    let first = first_token.unwrap_or(end);
    let gen_secs = end.duration_since(first).as_secs_f32();
    // the first step's token is produced at `first`, the rate counts the rest
    let tokens_per_sec = if generated > 1 && gen_secs > 0.0 {
        (generated - 1) as f32 / gen_secs
    } else {
        0.0
    };
    let stats = GenStats {
        prompt_tokens,
        tokens: generated as u32,
        tokens_per_sec,
        time_to_first_token_ms: first.duration_since(start).as_millis() as u64,
        total_ms: end.duration_since(start).as_millis() as u64,
        stop_reason,
        context_size: info.context_length,
    };
    Ok((text, stats))
}

#[cfg(test)]
mod tests {
    use super::drain_utf8;

    #[test]
    fn utf8_split_across_tokens() {
        let bytes = "aé€😀".as_bytes();
        for split in 0..bytes.len() {
            let mut pending = bytes[..split].to_vec();
            let mut out = drain_utf8(&mut pending);
            pending.extend_from_slice(&bytes[split..]);
            out.push_str(&drain_utf8(&mut pending));
            assert_eq!(out, "aé€😀");
            assert!(pending.is_empty());
        }
    }

    #[test]
    fn utf8_invalid_bytes_are_replaced() {
        let mut pending = vec![b'a', 0xff, b'b'];
        assert_eq!(drain_utf8(&mut pending), "a\u{fffd}b");
    }
}

/// Loads a real model: `SKELM_TEST_MODEL=qwen2.5:0.5b cargo test -- --ignored`
#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    #[ignore]
    fn generate_with_a_real_model() {
        let name = std::env::var("SKELM_TEST_MODEL").unwrap_or("qwen2.5:0.5b".into());
        let logs = crate::logs::init();
        let (engine, events) = Engine::spawn();
        engine.send(Command::Load {
            name,
            config: LoadConfig {
                context_length: 2048,
                ..LoadConfig::default()
            },
        });
        engine.send(Command::Generate(GenerateRequest {
            id: 1,
            messages: vec![Message::user("Say hello in five words.")],
            sampling: Sampling {
                max_tokens: Some(32),
                seed: Some(42),
                ..Sampling::default()
            },
            cancel: Arc::default(),
        }));
        let mut streamed = String::new();
        loop {
            match events.recv_blocking().unwrap() {
                Event::Loaded(info) => eprintln!("loaded {info:?}"),
                Event::LoadFailed { error, .. } => {
                    while let Ok(line) = logs.try_recv() {
                        eprintln!("[{:?} {}] {}", line.level, line.source, line.text);
                    }
                    panic!("load failed: {error}")
                }
                Event::Token { text, .. } => streamed.push_str(&text),
                Event::Finished { text, stats, .. } => {
                    eprintln!("{text:?} {stats:?}");
                    assert_eq!(text, streamed);
                    assert!(stats.tokens > 0);
                    break;
                }
                Event::Failed { error, .. } => panic!("generation failed: {error}"),
                _ => {}
            }
        }
    }
}

//! A process-wide log sink shown on the Developer page: llama.cpp messages,
//! `tracing` events (the API server's) and the studio's own.

use std::cell::RefCell;
use std::io::Write;
use std::sync::OnceLock;

use chrono::{DateTime, Local};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: DateTime<Local>,
    pub level: Level,
    pub source: &'static str,
    pub text: String,
}

static SINK: OnceLock<async_channel::Sender<LogLine>> = OnceLock::new();

/// Install the sink, returning the receiving end (call once)
pub fn init() -> async_channel::Receiver<LogLine> {
    let (tx, rx) = async_channel::unbounded();
    let _ = SINK.set(tx);

    skelm_llama_cpp::llama_logging(Box::new(|level, key, text: &str| {
        use skelm_llama_cpp::{LogKey, LogLevel};
        let level = match level {
            LogLevel::Error => Level::Error,
            LogLevel::Warn => Level::Warn,
            _ if key == LogKey::ModelLoader => Level::Info,
            _ => return,
        };
        let text = text.trim();
        if !text.is_empty() {
            if level == Level::Error {
                CAPTURE.with_borrow_mut(|c| {
                    if let Some(c) = c {
                        c.push(text.to_string());
                    }
                });
            }
            log(level, "llama.cpp", text);
        }
    }));

    // only the API server's events: GPUI and the HTTP stack log through
    // `log` too, and their internals don't belong on the Developer page
    use tracing_subscriber::{filter::Targets, layer::SubscriberExt as _, util::SubscriberInitExt as _};
    let targets = Targets::new()
        .with_target("skelm_api", tracing::Level::INFO)
        .with_target("axum", tracing::Level::WARN);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_writer(|| TracingWriter(Vec::new()));
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::Layer::with_filter(layer, targets))
        .try_init();

    rx
}

thread_local! {
    static CAPTURE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Run `f`, also returning the llama.cpp errors it logged on this thread:
/// skelm-core's own errors only say "consult logging for reasons"
pub fn capture_llama_errors<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let previous = CAPTURE.replace(Some(Vec::new()));
    let result = f();
    let errors = CAPTURE.replace(previous).unwrap_or_default();
    (result, errors)
}

/// The reasons llama.cpp gave for a failure, as one message: the generic
/// summary lines are dropped, and repeated prefixes trimmed
pub fn llama_reason(errors: &[String]) -> Option<String> {
    let reasons: Vec<&str> = errors
        .iter()
        .map(|e| {
            ["error loading model: ", "failed to initialize the context: "]
                .iter()
                .find_map(|p| e.strip_prefix(p))
                .unwrap_or(e)
        })
        .filter(|e| *e != "failed to load model")
        .collect();
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

pub fn log(level: Level, source: &'static str, text: impl Into<String>) {
    let line = LogLine {
        time: Local::now(),
        level,
        source,
        text: text.into(),
    };
    if let Some(sink) = SINK.get() {
        let _ = sink.try_send(line);
    }
}

pub fn info(source: &'static str, text: impl Into<String>) {
    log(Level::Info, source, text)
}

pub fn error(source: &'static str, text: impl Into<String>) {
    log(Level::Error, source, text)
}

/// Collects one `tracing` event and forwards it when dropped
struct TracingWriter(Vec<u8>);

impl Write for TracingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for TracingWriter {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.0);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let (level, text) = if let Some(t) = text.strip_prefix("ERROR ") {
            (Level::Error, t)
        } else if let Some(t) = text.strip_prefix("WARN ") {
            (Level::Warn, t)
        } else {
            let t = ["INFO ", "DEBUG ", "TRACE "]
                .iter()
                .find_map(|p| text.strip_prefix(p))
                .unwrap_or(text);
            (Level::Info, t)
        };
        log(level, "server", text.trim());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn llama_reason_keeps_the_cause() {
        let errors = [
            "error loading model: check_tensor_dims: tensor 'blk.0.ssm_dt.bias' not found".to_string(),
            "failed to load model".to_string(),
        ];
        assert_eq!(
            super::llama_reason(&errors).as_deref(),
            Some("check_tensor_dims: tensor 'blk.0.ssm_dt.bias' not found")
        );
        assert_eq!(super::llama_reason(&["failed to load model".into()]), None);
    }
}

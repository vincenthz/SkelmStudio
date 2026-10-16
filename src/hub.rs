//! Finding and downloading models: Hugging Face Hub search, repository
//! details and model cards, and pulls from the Hub or the Ollama registry.
//!
//! skelm's downloaders report progress through a [`ProgressDisplay`] type
//! constructed per file, with no way to pass it state. The download task runs
//! inside a task-local [`DownloadProgress`] scope that the per-file progress
//! values register with, and the UI polls its atomic counters.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use skelm_download::ProgressDisplay;
use skelm_hf::{HfClient, HfConfig, HfModelRef, ModelSummary, RepoId, RepoInfo, SearchQuery};
use skelm_ollama::{OllamaConfig, OllamaStore};

pub use skelm_hf::SearchSort;

#[derive(Default)]
struct FileProgress {
    total: AtomicU64,
    position: AtomicU64,
}

/// The progress of a download, shared with the UI
#[derive(Default)]
pub struct DownloadProgress {
    files: Mutex<Vec<Arc<FileProgress>>>,
    /// the size of the whole download when known up front
    expected: AtomicU64,
    pub finished: AtomicBool,
}

impl DownloadProgress {
    pub fn new(expected: Option<u64>) -> Arc<Self> {
        let p = Self::default();
        p.expected.store(expected.unwrap_or(0), Ordering::Relaxed);
        Arc::new(p)
    }

    /// (downloaded, total) bytes; the total is 0 when unknown
    pub fn snapshot(&self) -> (u64, u64) {
        let files = self.files.lock().unwrap();
        let mut position = 0;
        let mut total = 0;
        for f in files.iter() {
            position += f.position.load(Ordering::Relaxed);
            total += f.total.load(Ordering::Relaxed);
        }
        let expected = self.expected.load(Ordering::Relaxed);
        (position, total.max(expected))
    }

    fn start_file(&self, size: Option<u64>) -> Arc<FileProgress> {
        let file = Arc::new(FileProgress::default());
        file.total.store(size.unwrap_or(0), Ordering::Relaxed);
        self.files.lock().unwrap().push(file.clone());
        file
    }
}

tokio::task_local! {
    static PROGRESS: Arc<DownloadProgress>;
}

/// Reports a file's progress into the current task's [`DownloadProgress`]
pub struct GuiProgress(Option<Arc<FileProgress>>);

impl ProgressDisplay for GuiProgress {
    fn progress_start(size: Option<u64>) -> Self {
        GuiProgress(PROGRESS.try_with(|p| p.start_file(size)).ok())
    }

    fn progress_update(&self, position: u64) {
        if let Some(f) = &self.0 {
            f.position.store(position, Ordering::Relaxed);
        }
    }

    fn progress_finalize(self) {
        if let Some(f) = &self.0 {
            let total = f.total.load(Ordering::Relaxed);
            if total > 0 {
                f.position.store(total, Ordering::Relaxed);
            }
        }
    }
}

/// Download a model (`hf.co/owner/repo:QUANT` or an Ollama name), returning
/// the name it is installed under
pub async fn pull(name: String, progress: Arc<DownloadProgress>) -> anyhow::Result<String> {
    let result = PROGRESS
        .scope(progress.clone(), async move {
            if HfModelRef::is_hf_name(&name) {
                let model: HfModelRef = name.parse()?;
                let client = HfClient::from_env()?;
                let pulled = client.pull::<GuiProgress>(&model).await?;
                Ok(pulled.model.to_string())
            } else {
                let descr: skelm_ollama::ModelDescr = name
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!("invalid model name {name}: {e}"))?;
                let client = reqwest::Client::builder()
                    .user_agent(concat!("skelm-studio/", env!("CARGO_PKG_VERSION")))
                    .build()?;
                skelm_download::ollama::download_model::<GuiProgress>(
                    &client,
                    &OllamaConfig::default(),
                    &OllamaStore::default(),
                    &descr.registry,
                    &descr.model,
                    &descr.variant,
                )
                .await?;
                Ok(descr.to_string())
            }
        })
        .await;
    progress.finished.store(true, Ordering::Relaxed);
    result
}

pub async fn search(
    query: String,
    sort: SearchSort,
    limit: usize,
) -> anyhow::Result<Vec<ModelSummary>> {
    let client = HfClient::from_env()?;
    let query = query.trim();
    Ok(client
        .search_models(&SearchQuery {
            search: (!query.is_empty()).then(|| query.to_string()),
            author: None,
            gguf_only: true,
            sort,
            limit,
        })
        .await?)
}

pub async fn repo_info(repo: String) -> anyhow::Result<RepoInfo> {
    let repo: RepoId = repo.parse()?;
    let client = HfClient::from_env()?;
    Ok(client.repo_info(&repo, skelm_hf::DEFAULT_REVISION).await?)
}

/// The model card (README) of a repository, without its YAML front matter
pub async fn model_card(repo: String) -> anyhow::Result<String> {
    let config = HfConfig::from_env();
    let url = config
        .endpoint
        .join(&format!("{repo}/raw/{}/README.md", skelm_hf::DEFAULT_REVISION))?;
    let mut request = reqwest::Client::new().get(url);
    if let Some(token) = &config.token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        anyhow::bail!("no model card ({})", response.status());
    }
    Ok(strip_front_matter(&response.text().await?).to_string())
}

fn strip_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return text;
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            after.trim_start_matches(['\r', '\n'])
        }
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::strip_front_matter;

    #[test]
    fn front_matter() {
        assert_eq!(
            strip_front_matter("---\nlicense: mit\n---\n\n# Model\n"),
            "# Model\n"
        );
        assert_eq!(strip_front_matter("# Model\n"), "# Model\n");
    }
}

/// Downloads a small model: `HF_HUB_CACHE=/tmp/x cargo test -- --ignored pull_small`
#[cfg(test)]
mod network {
    use super::*;

    #[test]
    #[ignore]
    fn pull_small_model_with_progress() {
        crate::runtime::rt().block_on(async {
            let repos = search("stories15M".into(), SearchSort::Downloads, 10).await.unwrap();
            let mut pick = None;
            for repo in repos {
                let info = repo_info(repo.id.clone()).await.unwrap();
                if let Some(v) = info.gguf_variants().into_iter().min_by_key(|v| v.size()) {
                    if v.size() < 60_000_000 {
                        pick = Some((format!("hf.co/{}:{}", repo.id, v.quant), v.size()));
                        break;
                    }
                }
            }
            let (name, size) = pick.expect("a small GGUF");
            let progress = DownloadProgress::new(Some(size));
            let installed = pull(name.clone(), progress.clone()).await.unwrap();
            let (done, total) = progress.snapshot();
            eprintln!("{installed}: {done}/{total}");
            assert_eq!(total, size);
            assert_eq!(done, size);
            assert!(progress.finished.load(Ordering::Relaxed));
        });
    }
}

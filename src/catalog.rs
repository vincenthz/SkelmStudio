//! The locally installed models: the Ollama store (`~/.ollama`) and the
//! GGUF quantizations in the Hugging Face cache.

use std::path::PathBuf;
use std::time::SystemTime;

use skelm_exec::ModelDescr;
use skelm_hf::HfCache;
use skelm_ollama::{self as ollama, OllamaStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSource {
    Ollama,
    HuggingFace,
}

impl ModelSource {
    pub fn label(self) -> &'static str {
        match self {
            ModelSource::Ollama => "Ollama",
            ModelSource::HuggingFace => "Hugging Face",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalModel {
    /// the name skelm knows it by (`qwen2.5:7b`, `hf.co/owner/repo:Q4_K_M`)
    pub name: String,
    /// the model name alone (`qwen2.5`, `Qwen3.5-2B-GGUF`)
    pub family: String,
    /// who publishes it (`library`, the Hugging Face owner)
    pub publisher: String,
    /// the variant / quantization (`7b`, `Q4_K_M`)
    pub variant: String,
    pub source: ModelSource,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// the GGUF file loaded (the first shard of a split model)
    pub path: Option<PathBuf>,
}

/// Every installed model, most recently modified first
pub fn list_local() -> Vec<LocalModel> {
    let mut models = Vec::new();

    let store = OllamaStore::default();
    for descr in store.list_model_descrs().unwrap_or_default() {
        let path = store.manifest_registry_model_variant_path(
            &descr.registry,
            &descr.model,
            &descr.variant,
        );
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let Ok(manifest) = store.get_manifest(&descr) else {
            continue;
        };
        let blob = manifest
            .find_media_type(ollama::MEDIA_TYPE_IMAGE_MODEL)
            .map(|layer| store.blob_path(&layer.digest));
        models.push(LocalModel {
            name: descr.to_string(),
            family: descr.model.as_str().to_string(),
            publisher: "ollama".into(),
            variant: descr.variant.as_str().to_string(),
            source: ModelSource::Ollama,
            size: manifest.size(),
            modified,
            path: blob,
        });
    }

    for cached in HfCache::from_env().list_models() {
        let repo = cached.model.repo.as_str();
        let (publisher, family) = repo.split_once('/').unwrap_or(("", repo));
        models.push(LocalModel {
            name: cached.model.to_string(),
            family: family.to_string(),
            publisher: publisher.to_string(),
            variant: cached.model.quant.clone().unwrap_or_default(),
            source: ModelSource::HuggingFace,
            size: cached.size,
            modified: cached.modified,
            path: Some(cached.path),
        });
    }

    models.sort_by(|a, b| b.modified.cmp(&a.modified));
    models
}

/// Delete an installed model
pub fn remove(name: &str) -> anyhow::Result<()> {
    match ModelDescr::parse(name).map_err(anyhow::Error::msg)? {
        ModelDescr::Ollama(descr) => {
            OllamaStore::default().remove_manifest(&descr.registry, &descr.model, &descr.variant)?
        }
        ModelDescr::Hf(model) => {
            HfCache::from_env().remove(&model)?;
        }
        ModelDescr::Path(_) => anyhow::bail!("{name} is a file, not an installed model"),
    }
    Ok(())
}

/// What the GGUF metadata says about a model
#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub architecture: Option<String>,
    pub name: Option<String>,
    pub size_label: Option<String>,
    pub context_length: Option<u64>,
    pub description: String,
    pub license: Option<String>,
    pub chat_template: Option<String>,
    pub vocab_size: u32,
}

impl ModelInfo {
    /// Whether the chat template knows about tools
    pub fn supports_tools(&self) -> bool {
        self.chat_template.as_deref().is_some_and(|t| t.contains("tools"))
    }

    /// Whether the chat template has a thinking / reasoning mode
    pub fn supports_reasoning(&self) -> bool {
        self.chat_template
            .as_deref()
            .is_some_and(|t| t.contains("<think>") || t.contains("reasoning_content"))
    }
}

/// Read the metadata of a model without loading its weights (blocking)
pub fn inspect(name: &str) -> anyhow::Result<ModelInfo> {
    let descr = ModelDescr::parse(name).map_err(anyhow::Error::msg)?;
    let params = skelm_exec::ModelParams {
        vocab_only: true,
        ..Default::default()
    };
    let model = skelm_exec::Model::load_with_params(&descr, &params)?;
    let m = &model.model;
    let arch = m.architecture();
    let meta = |key: &str| m.meta_val_str(key).filter(|v| !v.is_empty());
    let context_length = arch
        .as_ref()
        .and_then(|a| meta(&format!("{a}.context_length")))
        .and_then(|v| v.parse().ok());
    Ok(ModelInfo {
        name: meta("general.name"),
        size_label: meta("general.size_label"),
        license: meta("general.license"),
        context_length,
        description: m.description().trim_end_matches('\0').to_string(),
        chat_template: m.chat_template(),
        vocab_size: model.vocab.n_tokens(),
        architecture: arch,
    })
}

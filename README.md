# Skelm Studio

A desktop studio for local LLMs, built on [skelm-core](https://github.com/vincenthz/skelm-core) (llama.cpp) and [GPUI Component](https://gpui-kit.com) 0.7.

```sh
cargo run --release
```

skelm-core is fetched from git (with its llama.cpp submodule), at the commit
pinned in `Cargo.lock`; `cargo update` moves it to the latest `master`. To
build against a local checkout instead, add to `.cargo/config.toml`:

```toml
[patch."https://github.com/vincenthz/skelm-core"]
skelm-api = { path = "../skelm-core/crates/skelm-api" }
skelm-download = { path = "../skelm-core/crates/skelm-download" }
skelm-exec = { path = "../skelm-core/crates/skelm-exec" }
skelm-hf = { path = "../skelm-core/crates/skelm-hf" }
skelm-llama-cpp = { path = "../skelm-core/crates/skelm-llama-cpp" }
skelm-ollama = { path = "../skelm-core/crates/skelm-ollama" }
```

## Bundle

With [cargo-bundle](https://github.com/burtonageo/cargo-bundle)
(`cargo install cargo-bundle`):

```sh
cargo bundle --release
```

This builds `target/release/bundle/osx/Skelm Studio.app` and a `.dmg` (a
`.deb` on Linux). The app is self-contained: llama.cpp is linked statically,
with its Metal shaders embedded. It isn't signed, so a copy downloaded from
elsewhere needs a right-click › Open the first time. An app started from the
Finder doesn't see your shell's environment: for gated repositories, log in
with `huggingface-cli login` (its saved token is used) rather than relying on
`HF_TOKEN`.

The icon is drawn in `assets/icon/icon.svg`; after changing it, regenerate
the bundle's PNGs with `cargo run --example render_icon`.

## Pages

The rail on the left switches between:

- **Chat**: the chat list, the conversation and the settings panel.
  - Markdown rendering with syntax-highlighted code blocks (with copy
    buttons), tables, streamed text fade-in, and collapsible reasoning blocks
    (`<think>…</think>`, "Thought for 3.2 seconds").
  - Each reply shows its tok/s, token count, time to first token and stop reason.
    The composer shows how full the context window is.
  - Message actions: copy, edit (and resubmit), regenerate, branch into a new
    chat, delete. Text files can be attached to a message.
  - Chats can be searched, pinned, renamed, duplicated, exported as Markdown,
    and grouped by date.
  - **Projects** group chats. Each project has instructions, reference files
    (given to the model in every chat of the project), and a default model that
    loads when one of its chats is opened.
  - The settings panel holds the system prompt, temperature, the response
    length limit, top-k / top-p / min-p, repeat penalty and seed, per chat. The
    current settings can be saved as the default for new chats.
- **Developer**: an OpenAI-compatible server (`skelm-api`) with start/stop,
  host/port, endpoints, a curl example, and the logs (llama.cpp loader, requests).
- **My Models**: the installed models (Ollama store and Hugging Face cache) with
  size, source and age. A details panel shows the GGUF metadata (architecture,
  parameters, trained context, license), capabilities (tools, reasoning) and the
  chat template, with buttons to load, reveal in Finder, or delete.
- **Discover**: search GGUF repositories on the Hugging Face Hub (trending,
  downloads, likes, recent), see a repository's quantizations, sizes and model
  card, and download one. Models can also be pulled by name from the Ollama
  registry (`qwen2.5:7b`) or the Hub (`hf.co/owner/repo:Q4_K_M`). Downloads
  show progress and speed, can be cancelled, and resume.

The model loader at the top (⌘L) picks the model to load and how: context
length, GPU offload, flash attention, memory-mapping and MTP speculative
decoding. These settings are remembered per model.

Shortcuts: ⌘L model loader, ⌘N new chat, ⌘B chat list, ⌘. settings panel,
⌘E eject, ⏎ send / ⇧⏎ new line.

## Data

Chats, projects and settings are JSON files under
`~/Library/Application Support/skelm-studio` (`$XDG_DATA_HOME/skelm-studio` on
Linux); set `SKELM_STUDIO_HOME` to use another directory. Models live in
`~/.ollama` and the Hugging Face cache, shared with the `skelm` CLI.
`HF_TOKEN` is honoured for gated repositories.

## Layout

| Module | |
|---|---|
| `engine` | the inference thread: load/unload, streaming generation with stats and cancellation, the sampler chain |
| `catalog` | installed models, GGUF metadata, deletion |
| `hub` | Hub search, repository info, model cards, downloads with progress |
| `server` | the local API server |
| `store` | persisted chats, projects and settings |
| `conversation` | transcript building (projects, attachments), reasoning split, titles, Markdown export |
| `ui` | the window: `Studio` owns the state, one module per page |

## Development

```sh
cargo test                                   # unit tests, and the API server
SKELM_TEST_MODEL=qwen2.5:0.5b cargo test -- --ignored generate   # real generation
HF_HUB_CACHE=/tmp/hf cargo test -- --ignored pull_small          # real download

# a scripted tour of the UI, saving a screenshot of each page (rendered from
# the window's own scene, so no screen-recording permission is needed)
SKELM_STUDIO_HOME=/tmp/studio SKELM_STUDIO_TOUR=/tmp/shots \
    cargo run --features dev-tour
```

## Known limitations

- Each reply renders the whole conversation into a fresh context, as
  `skelm run` and `skelm serve` do. skelm-core doesn't yet expose a way to roll
  a context back to a shared prefix, so long chats reprocess their prompt each
  turn.
- One model is loaded at a time. The API server loads its own copies on demand.
- Images in model cards don't load (the app has no GPUI HTTP client yet), and
  there is no vision input.

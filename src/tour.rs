//! A scripted walk through the UI that saves a screenshot of each step, for
//! development (feature `dev-tour`):
//!
//! ```sh
//! SKELM_STUDIO_TOUR=/tmp/shots SKELM_STUDIO_TOUR_MODEL=qwen2.5:0.5b \
//!     cargo run --features dev-tour
//! ```
//!
//! Screenshots are rendered from the window's own scene, so they need no
//! screen-recording permission.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{AsyncWindowContext, WeakEntity, Window};

use crate::ui::{ModelState, Page, Studio};

pub fn start(studio: WeakEntity<Studio>, window: &mut Window, cx: &mut gpui::App) {
    let Some(dir) = std::env::var_os("SKELM_STUDIO_TOUR").map(PathBuf::from) else {
        return;
    };
    let model = std::env::var("SKELM_STUDIO_TOUR_MODEL").unwrap_or_else(|_| "qwen2.5:0.5b".into());
    std::fs::create_dir_all(&dir).expect("tour directory");
    window
        .spawn(cx, async move |cx| {
            if let Err(e) = tour(studio, dir, model, cx).await {
                eprintln!("tour failed: {e:#}");
            }
            let _ = cx.update(|_, cx| cx.quit());
        })
        .detach();
}

async fn wait(cx: &mut AsyncWindowContext, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

/// Wait until `done` holds (checked every 250ms), at most `secs`
async fn wait_for(
    studio: &WeakEntity<Studio>,
    cx: &mut AsyncWindowContext,
    secs: u64,
    done: impl Fn(&Studio) -> bool,
) -> anyhow::Result<bool> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if studio.update(cx, |s, _| done(s))? {
            return Ok(true);
        }
        wait(cx, 250).await;
    }
    Ok(false)
}

thread_local! {
    static LAST_FRAME: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

fn shot(dir: &PathBuf, name: &str, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
    cx.update(|window, cx| {
        window.refresh();
        let _ = cx;
        let image = window.render_to_image()?;
        let path = dir.join(format!("{name}.png"));
        image.save(&path)?;
        eprintln!("tour: saved {}", path.display());
        // macOS stops drawing while the display sleeps or the screen is
        // locked, leaving the last frame in every screenshot
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            image.as_raw().hash(&mut h);
            h.finish()
        };
        if LAST_FRAME.replace(Some(hash)) == Some(hash) {
            eprintln!("tour: warning: {name} is the same frame as the last one (display asleep or screen locked?)");
        }
        anyhow::Ok(())
    })?
}

async fn tour(
    studio: WeakEntity<Studio>,
    dir: PathBuf,
    model: String,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    wait(cx, 2500).await;
    shot(&dir, "01-chat-empty", cx)?;

    studio.update_in(cx, |s, window, cx| s.open_picker(window, cx))?;
    wait(cx, 600).await;
    studio.update_in(cx, |s, window, cx| s.picker_select(model.clone(), window, cx))?;
    wait_for(&studio, cx, 10, |s| s.model_info.contains_key(&model)).await?;
    wait(cx, 400).await;
    shot(&dir, "02-model-picker", cx)?;

    studio.update(cx, |s, cx| s.picker_load(cx))?;
    let loaded = wait_for(&studio, cx, 120, |s| {
        matches!(s.model_state, ModelState::Loaded(_) | ModelState::Unloaded)
    })
    .await?;
    if !loaded || studio.update(cx, |s, _| s.model_state.loaded().is_none())? {
        // the error notification, then where its details are
        wait(cx, 500).await;
        shot(&dir, "03-load-failed", cx)?;
        studio.update_in(cx, |s, window, cx| s.set_page(Page::Developer, window, cx))?;
        wait(cx, 800).await;
        shot(&dir, "04-load-failed-logs", cx)?;
        anyhow::bail!("{model} did not load");
    }

    let prompt = "Compare three planets in a markdown table (name, diameter, moons), \
                  then show a Rust function that prints them. Be brief.";
    studio.update_in(cx, |s, window, cx| {
        s.composer.update(cx, |c, cx| c.set_value(prompt, window, cx));
        s.send_message(window, cx);
    })?;
    wait(cx, 1500).await;
    shot(&dir, "03-chat-streaming", cx)?;
    wait_for(&studio, cx, 180, |s| s.generation.is_none()).await?;
    wait(cx, 800).await;
    shot(&dir, "04-chat-reply", cx)?;

    // copying the chat puts its Markdown on the clipboard (restored after)
    let copied = studio.update_in(cx, |s, window, cx| {
        let previous = cx.read_from_clipboard();
        let id = s.active_chat.clone().unwrap_or_default();
        s.copy_chat(&id, window, cx);
        let copied = cx.read_from_clipboard().and_then(|c| c.text());
        if let Some(previous) = previous {
            cx.write_to_clipboard(previous);
        }
        copied
    })?;
    match copied {
        Some(text) if text.contains("## You") && text.contains("planets") => {
            eprintln!("tour: copied the chat ({} bytes)", text.len())
        }
        other => anyhow::bail!("copying the chat gave {other:?}"),
    }

    studio.update_in(cx, |s, window, cx| {
        s.set_page(Page::Models, window, cx);
        s.selected_model = Some(model.clone());
        s.inspect_model(&model, cx);
    })?;
    wait(cx, 1500).await;
    shot(&dir, "05-my-models", cx)?;

    studio.update_in(cx, |s, window, cx| s.set_page(Page::Discover, window, cx))?;
    wait_for(&studio, cx, 30, |s| !s.discover.searching && s.discover.repo.is_some()).await?;
    wait_for(&studio, cx, 20, |s| s.discover.card.is_some()).await?;
    wait(cx, 800).await;
    shot(&dir, "06-discover", cx)?;

    studio.update_in(cx, |s, window, cx| {
        s.set_page(Page::Developer, window, cx);
        s.toggle_server(window, cx);
    })?;
    wait_for(&studio, cx, 10, |s| s.server.is_some()).await?;
    wait(cx, 800).await;
    shot(&dir, "07-developer", cx)?;

    studio.update_in(cx, |s, window, cx| {
        s.toggle_server(window, cx);
        s.new_project(window, cx);
    })?;
    wait(cx, 800).await;
    shot(&dir, "08-project", cx)?;

    studio.update_in(cx, |s, window, cx| s.toggle_theme(window, cx))?;
    wait(cx, 800).await;
    shot(&dir, "09-light-theme", cx)?;
    studio.update_in(cx, |s, window, cx| s.toggle_theme(window, cx))?;
    wait(cx, 300).await;
    Ok(())
}

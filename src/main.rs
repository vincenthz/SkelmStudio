//! Skelm Studio: a desktop app to find, download, manage and chat with local
//! LLMs, on top of skelm-core (llama.cpp) and GPUI Component.
//!
//! Data lives under `~/Library/Application Support/skelm-studio` (override
//! with `SKELM_STUDIO_HOME`); models in the Ollama store (`~/.ollama`) and
//! the Hugging Face cache, shared with the `skelm` CLI.

mod assets;
mod catalog;
mod conversation;
mod engine;
mod hub;
mod logs;
mod runtime;
mod server;
mod store;
#[cfg(feature = "dev-tour")]
mod tour;
mod ui;

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, Menu, MenuItem, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, Theme, ThemeMode, TitleBar};

fn main() {
    let logs_rx = logs::init();
    let store = match store::Store::open() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("cannot open the data directory: {e:#}");
            std::process::exit(1);
        }
    };
    // start the network runtime before the UI needs it
    let _ = runtime::rt();
    logs::info(
        "studio",
        format!("data in {}", store.root().display()),
    );

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            ui::bind_keys(cx);
            cx.set_menus([Menu {
                name: "Skelm Studio".into(),
                items: vec![MenuItem::action("Quit Skelm Studio", ui::Quit)],
                disabled: false,
            }]);

            let dark = store.load_settings().dark_mode;
            Theme::change(
                if dark { ThemeMode::Dark } else { ThemeMode::Light },
                None,
                cx,
            );

            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.), px(600.))),
                ..TitleBar::window_options()
            };
            let store = store.clone();
            let logs_rx = logs_rx.clone();
            cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| ui::Studio::new(store, logs_rx, window, cx));
                #[cfg(feature = "dev-tour")]
                tour::start(view.downgrade(), window, cx);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the main window");
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
}

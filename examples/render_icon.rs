//! Render `assets/icon/icon.svg` to the PNGs of the app bundle (the sizes of
//! a macOS `.icns`, and the Linux icons):
//!
//! ```sh
//! cargo run --example render_icon
//! ```

use std::path::Path;

use resvg::{tiny_skia, usvg};

/// (file name, pixel size)
const SIZES: &[(&str, u32)] = &[
    ("16x16", 16),
    ("16x16@2x", 32),
    ("32x32", 32),
    ("32x32@2x", 64),
    ("128x128", 128),
    ("128x128@2x", 256),
    ("256x256", 256),
    ("256x256@2x", 512),
    ("512x512", 512),
    ("512x512@2x", 1024),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon");
    let svg = std::fs::read(dir.join("icon.svg"))?;
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())?;
    let source = tree.size();
    for &(name, size) in SIZES {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("bad icon size")?;
        let scale = size as f32 / source.width().max(source.height());
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let path = dir.join(format!("{name}.png"));
        pixmap.save_png(&path)?;
        println!("{}", path.display());
    }
    Ok(())
}

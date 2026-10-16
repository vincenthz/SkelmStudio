//! The application's assets: GPUI Component's own icons, plus the extra
//! Lucide icons the studio uses.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub use gpui_kit_assets::IconName as Lucide;

gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [
        Box,
        Brain,
        CircleStop,
        ClipboardCopy,
        Clock,
        CodeXml,
        Download,
        FilePlus,
        FolderInput,
        FolderPlus,
        Gauge,
        GitBranch,
        HardDriveDownload,
        Hash,
        Layers,
        Library,
        Lightbulb,
        ListFilter,
        MessageSquare,
        MessageSquarePlus,
        MessagesSquare,
        Package,
        Paperclip,
        Pencil,
        Pin,
        PinOff,
        Plug,
        Power,
        RotateCcw,
        Scale,
        ScrollText,
        Send,
        Server,
        SlidersHorizontal,
        Sparkles,
        SquarePen,
        Telescope,
        Terminal,
        Trash,
        TrendingUp,
        Unplug,
        Upload,
        Wrench,
        Zap
    ]
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

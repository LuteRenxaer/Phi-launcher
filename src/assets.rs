//! Locating and loading bundled assets (icon, background, fonts, sounds).
//!
//! Assets live in an `assets` directory. We try, in order: the current
//! working directory, the directory next to the executable, and finally the
//! compile-time manifest dir (useful during `cargo run`).

use std::path::{Path, PathBuf};

/// Some GPUs / GL backends cap texture side length (this machine reports
/// 2048). Clamp any image we upload as a texture to stay safely within it.
const MAX_TEXTURE_SIDE: u32 = 2048;

/// Resolve the `assets` directory by probing likely locations.
pub fn assets_dir() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets"));
            if let Some(up) = dir.parent().and_then(|p| p.parent()) {
                candidates.push(up.join("assets"));
            }
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));

    for c in &candidates {
        if c.is_dir() {
            return c.clone();
        }
    }
    PathBuf::from("assets")
}

/// The launcher's base directory (where `versions/` will live).
pub fn base_dir() -> PathBuf {
    assets_dir()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Load the window icon from `assets/icon.png`.
pub fn load_icon(assets: &Path) -> Option<egui::IconData> {
    let bytes = std::fs::read(assets.join("icon.png")).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

/// Load a raw image file into an egui `ColorImage`, downscaling if it exceeds
/// the max texture side so texture upload never panics.
pub fn load_color_image(path: &Path) -> Option<egui::ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    let mut img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
        let scale = MAX_TEXTURE_SIDE as f32 / w.max(h) as f32;
        let nw = (w as f32 * scale).floor().max(1.0) as u32;
        let nh = (h as f32 * scale).floor().max(1.0) as u32;
        img = img.resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    }
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    ))
}

/// The named font family used for the Latin brand title ("Phi Launcher").
pub const BRAND_FAMILY: &str = "brand";

/// Find a usable Windows CJK font (single-file TTFs preferred to avoid
/// `.ttc` collection indexing issues).
fn load_cjk_font() -> Option<Vec<u8>> {
    let candidates = [
        r"C:\Windows\Fonts\deng.ttf",   // DengXian 等线
        r"C:\Windows\Fonts\simhei.ttf", // SimHei 黑体
        r"C:\Windows\Fonts\msyh.ttc",   // Microsoft YaHei 微软雅黑
        r"C:\Windows\Fonts\simsun.ttc", // SimSun 宋体
    ];
    for c in candidates {
        if let Ok(bytes) = std::fs::read(c) {
            return Some(bytes);
        }
    }
    None
}

/// Install fonts:
/// * A real Windows CJK font is the primary text font (correct, un-garbled
///   Chinese — the bundled `phigros.ttf` is a renamed subset whose CJK
///   outlines render as tofu on some systems).
/// * `phigros.ttf` is registered under the [`BRAND_FAMILY`] family and used
///   only for the Latin brand title, preserving the Phigros look.
pub fn install_fonts(ctx: &egui::Context, assets: &Path) {
    use egui::{FontData, FontFamily};

    let mut fonts = egui::FontDefinitions::default();

    let have_cjk = if let Some(bytes) = load_cjk_font() {
        fonts.font_data.insert("cjk".to_owned(), FontData::from_owned(bytes));
        true
    } else {
        false
    };

    let phigros_bytes = std::fs::read(assets.join("phigros.ttf"))
        .or_else(|_| std::fs::read(assets.join("font.ttf")))
        .or_else(|_| std::fs::read(assets.join("bold.ttf")))
        .ok();
    let have_phigros = if let Some(bytes) = phigros_bytes {
        fonts.font_data.insert("phigros".to_owned(), FontData::from_owned(bytes));
        true
    } else {
        false
    };

    // Proportional (body) text: CJK font first for correct glyphs; fall back
    // to phigros only if no system CJK font was found.
    {
        let prop = fonts.families.entry(FontFamily::Proportional).or_default();
        if have_cjk {
            prop.insert(0, "cjk".to_owned());
        } else if have_phigros {
            prop.insert(0, "phigros".to_owned());
        }
    }
    {
        let mono = fonts.families.entry(FontFamily::Monospace).or_default();
        if have_cjk {
            mono.push("cjk".to_owned());
        }
    }

    // Brand family for the Latin title: phigros first, CJK as fallback.
    let mut brand = Vec::new();
    if have_phigros {
        brand.push("phigros".to_owned());
    }
    if have_cjk {
        brand.push("cjk".to_owned());
    }
    if !brand.is_empty() {
        fonts
            .families
            .insert(FontFamily::Name(BRAND_FAMILY.into()), brand);
    }

    ctx.set_fonts(fonts);
}

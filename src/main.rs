#![windows_subsystem = "windows"]

mod app;
mod git_ops;
mod recent;
mod ui;
mod updater;

use eframe::egui;
use std::path::{Path, PathBuf};

/// Included via build.rs — provides VERSION, GIT_HASH, GIT_DESCRIBE, BUILD_DATE constants.
mod version_info {
    include!(concat!(env!("OUT_DIR"), "/version_info.rs"));
}

/// Configure fonts with system font fallbacks for broad Unicode/emoji coverage.
///
/// egui's default fonts do not include every emoji glyph or Unicode symbol used by
/// the UI (↑ ↓ ▶ 📂 🔀 🗑 etc.). This function tries to load platform system fonts
/// and adds them as fallbacks so missing characters render instead of as boxes.
fn configure_fonts(cc: &eframe::CreationContext) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_fonts = std::collections::HashSet::new();

    // Try to load system fonts for Unicode/emoji coverage, gracefully ignoring failures.
    // The default egui fonts are bundled with the application, but their glyph coverage
    // differs from the fonts users have available on each supported platform.
    for (path, name) in system_font_candidates() {
        if loaded_fonts.contains(name) {
            continue;
        }
        if try_add_font(&mut fonts, &path, name) {
            loaded_fonts.insert(name);
        }
    }

    cc.egui_ctx.set_fonts(fonts);
}

/// Return common Unicode-capable fonts for the supported desktop platforms.
///
/// The paths are candidates rather than requirements: installations may omit any
/// of these fonts, and `configure_fonts` simply skips paths that are unavailable.
fn system_font_candidates() -> Vec<(PathBuf, &'static str)> {
    let mut candidates = Vec::new();

    #[cfg(windows)]
    {
        let system_root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("WINDIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let system_font_dir = system_root.join("Fonts");

        candidates.push((system_font_dir.join("seguiemj.ttf"), "SegoeUIEmoji"));
        candidates.push((system_font_dir.join("seguisym.ttf"), "SegoeUISymbol"));

    }

    #[cfg(target_os = "macos")]
    {
        candidates.extend([
            (
                PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
                "ArialUnicode",
            ),
            (
                PathBuf::from("/System/Library/Fonts/Apple Symbols.ttf"),
                "AppleSymbols",
            ),
            (
                PathBuf::from("/Library/Fonts/Arial Unicode.ttf"),
                "ArialUnicode",
            ),
            (
                PathBuf::from("/Library/Fonts/Arial.ttf"),
                "Arial",
            ),
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        candidates.extend([
            (
                PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
                "DejaVuSans",
            ),
            (
                PathBuf::from("/usr/share/fonts/dejavu/DejaVuSans.ttf"),
                "DejaVuSans",
            ),
            (
                PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"),
                "NotoSansSymbols",
            ),
            (
                PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansSymbols2-Regular.ttf"),
                "NotoSansSymbols",
            ),
            (
                PathBuf::from("/usr/share/fonts/google-noto-vf/NotoSansSymbols[wght].ttf"),
                "NotoSansSymbols",
            ),
            (
                PathBuf::from("/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf"),
                "NotoEmoji",
            ),
            (
                PathBuf::from("/usr/share/fonts/google-noto-emoji/NotoEmoji-Regular.ttf"),
                "NotoEmoji",
            ),
        ]);
    }

    candidates
}

/// Try to load a font from `path` and add it as a fallback for all font families.
/// Silently ignores failures (file not found, invalid font, etc.).
fn try_add_font(fonts: &mut egui::FontDefinitions, path: &Path, name: &str) -> bool {
    if let Ok(data) = std::fs::read(path) {
        let name = name.to_owned();
        fonts
            .font_data
            .insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        // Add as fallback for all font families
        for family in fonts.families.values_mut() {
            if !family.contains(&name) {
                family.push(name.clone());
            }
        }
        true
    } else {
        false
    }
}

fn native_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select a Git repository")
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

fn main() -> eframe::Result<()> {
    let app_title = format!("Git Manager v{}", version_info::VERSION);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 550.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title(&app_title),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "Git Manager",
        options,
        Box::new(|cc| {
            configure_fonts(cc);
            Ok(Box::new(app::App::new()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_font_candidates_include_platform_fallbacks() {
        let candidates = system_font_candidates();
        let names: Vec<&str> = candidates.iter().map(|(_, name)| *name).collect();

        #[cfg(windows)]
        {
            assert!(names.contains(&"SegoeUIEmoji"));
            assert!(names.contains(&"SegoeUISymbol"));
        }

        #[cfg(target_os = "macos")]
        {
            assert!(names.contains(&"ArialUnicode"));
            assert!(names.contains(&"AppleSymbols"));
        }

        #[cfg(target_os = "linux")]
        {
            assert!(names.contains(&"DejaVuSans"));
            assert!(names.contains(&"NotoSansSymbols"));
            assert!(names.contains(&"NotoEmoji"));
        }

        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        assert!(names.is_empty(), "unsupported platforms should not assume font paths");
    }
}

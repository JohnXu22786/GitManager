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

    // Try to load system fonts for Unicode/emoji coverage, gracefully ignoring failures.
    // The default egui fonts are bundled with the application, but their glyph coverage
    // differs from the fonts users have available on each supported platform.
    load_font_fallbacks(&mut fonts, system_font_candidates());

    cc.egui_ctx.set_fonts(fonts);
}

fn load_font_fallbacks(
    fonts: &mut egui::FontDefinitions,
    candidates: impl IntoIterator<Item = (PathBuf, &'static str)>,
) -> usize {
    let mut loaded_fonts = std::collections::HashSet::new();
    for (path, name) in candidates {
        if loaded_fonts.contains(name) {
            continue;
        }
        if try_add_font(fonts, &path, name) {
            loaded_fonts.insert(name);
        }
    }
    loaded_fonts.len()
}

/// Return common Unicode-capable fonts for the supported desktop platforms.
///
/// The paths are candidates rather than requirements: installations may omit any
/// of these fonts, and `configure_fonts` simply skips paths that are unavailable.
fn system_font_candidates() -> Vec<(PathBuf, &'static str)> {
    platform_font_candidates(current_font_platform())
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum FontPlatform {
    Windows,
    MacOS,
    Linux,
    Unsupported,
}

fn current_font_platform() -> FontPlatform {
    #[cfg(windows)]
    {
        return FontPlatform::Windows;
    }

    #[cfg(target_os = "macos")]
    {
        return FontPlatform::MacOS;
    }

    #[cfg(target_os = "linux")]
    {
        return FontPlatform::Linux;
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    FontPlatform::Unsupported
}

fn platform_font_candidates(platform: FontPlatform) -> Vec<(PathBuf, &'static str)> {
    match platform {
        FontPlatform::Windows => {
            let system_root = std::env::var_os("SystemRoot")
                .or_else(|| std::env::var_os("WINDIR"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let system_font_dir = system_root.join("Fonts");

            vec![
                (system_font_dir.join("seguiemj.ttf"), "SegoeUIEmoji"),
                (system_font_dir.join("seguisym.ttf"), "SegoeUISymbol"),
            ]
        }

        FontPlatform::MacOS => vec![
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
            (PathBuf::from("/Library/Fonts/Arial.ttf"), "Arial"),
        ],

        FontPlatform::Linux => vec![
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
                PathBuf::from("/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf"),
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
            (
                PathBuf::from("/usr/share/fonts/google-noto-emoji-fonts/NotoEmoji-Regular.ttf"),
                "NotoEmoji",
            ),
        ],

        FontPlatform::Unsupported => Vec::new(),
    }
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
    use std::io::Write;

    #[test]
    fn system_font_candidates_include_platform_fallbacks() {
        for (platform, expected_names) in [
            (
                FontPlatform::Windows,
                ["SegoeUIEmoji", "SegoeUISymbol"].as_slice(),
            ),
            (
                FontPlatform::MacOS,
                ["ArialUnicode", "AppleSymbols"].as_slice(),
            ),
            (
                FontPlatform::Linux,
                ["DejaVuSans", "NotoSansSymbols", "NotoEmoji"].as_slice(),
            ),
        ] {
            let names: Vec<&str> = platform_font_candidates(platform)
                .iter()
                .map(|(_, name)| *name)
                .collect();
            for expected_name in expected_names {
                assert!(
                    names.contains(expected_name),
                    "{platform:?} should include {expected_name}"
                );
            }
        }
    }

    #[test]
    fn linux_font_candidates_include_common_noto_installations() {
        let paths: Vec<PathBuf> = platform_font_candidates(FontPlatform::Linux)
            .into_iter()
            .map(|(path, _)| path)
            .collect();

        for expected in [
            "/usr/share/fonts/google-noto-emoji-fonts/NotoEmoji-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
        ] {
            assert!(
                paths.iter().any(|path| path == Path::new(expected)),
                "Linux font candidates should include {expected}"
            );
        }
    }

    #[test]
    fn try_add_font_loads_registers_and_renders_emoji() {
        let defaults = egui::FontDefinitions::default();
        let font_bytes = defaults
            .font_data
            .get("NotoEmoji-Regular")
            .expect("egui should provide a valid emoji test font")
            .font
            .to_vec();
        let mut font_file = tempfile::NamedTempFile::new().expect("create temporary font file");
        font_file
            .write_all(&font_bytes)
            .expect("write temporary font file");

        let mut fonts = egui::FontDefinitions::empty();
        let loaded = load_font_fallbacks(
            &mut fonts,
            vec![(font_file.path().to_path_buf(), "TestEmoji")],
        );
        assert_eq!(loaded, 1);
        assert_eq!(
            fonts.font_data["TestEmoji"].font.as_ref(),
            font_bytes.as_slice()
        );
        for family in fonts.families.values() {
            assert!(family.iter().any(|name| name == "TestEmoji"));
        }

        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        ctx.begin_pass(egui::RawInput::default());
        let galley = ctx.fonts(|fonts| {
            fonts.layout_no_wrap(
                "🚀".to_owned(),
                egui::FontId::proportional(16.0),
                egui::Color32::WHITE,
            )
        });
        assert!(galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .any(|glyph| glyph.chr == '🚀' && !glyph.uv_rect.is_nothing()));
        let _ = ctx.end_pass();
    }
}

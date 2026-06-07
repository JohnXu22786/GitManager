#![windows_subsystem = "windows"]

mod app;
mod git_ops;
mod recent;
mod ui;
mod updater;

use eframe::egui;

/// Included via build.rs — provides VERSION, GIT_HASH, GIT_DESCRIBE, BUILD_DATE constants.
mod version_info {
    include!(concat!(env!("OUT_DIR"), "/version_info.rs"));
}

/// Configure fonts with system font fallbacks for broad Unicode/emoji coverage.
///
/// egui's default fonts (Ubuntu-Light, Hack) do not include emoji glyphs or many
/// Unicode symbols (↑ ↓ ▶ 📂 🔀 🗑 etc.). This function tries to load system fonts
/// (e.g., Segoe UI Emoji, Segoe UI Symbol on Windows) and adds them as fallbacks
/// so that all Unicode characters used in the UI render correctly instead of as boxes.
fn configure_fonts(cc: &eframe::CreationContext) {
    let mut fonts = egui::FontDefinitions::default();

    // Try to load system fonts for Unicode/emoji coverage, gracefully ignoring failures
    #[cfg(windows)]
    {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let font_dir = format!("{}\\Fonts\\", system_root);

        // Segoe UI Emoji — provides emoji glyphs (Windows 8.1+)
        try_add_font(&mut fonts, &format!("{}seguiemj.ttf", font_dir), "SegoeUIEmoji");
        // Segoe UI Symbol — provides Unicode symbols/arrows (Windows 7+)
        try_add_font(&mut fonts, &format!("{}seguisym.ttf", font_dir), "SegoeUISymbol");
    }

    cc.egui_ctx.set_fonts(fonts);
}

/// Try to load a font from `path` and add it as a fallback for all font families.
/// Silently ignores failures (file not found, invalid font, etc.).
fn try_add_font(fonts: &mut egui::FontDefinitions, path: &str, name: &str) {
    if let Ok(data) = std::fs::read(path) {
        fonts
            .font_data
            .insert(name.to_owned(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        // Add as fallback for all font families
        for family in fonts.families.values_mut() {
            if !family.contains(&name.to_owned()) {
                family.push(name.to_owned());
            }
        }
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

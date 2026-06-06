pub mod status_panel;
pub mod branch_panel;
pub mod worktree_panel;
pub mod log_panel;
pub mod stash_panel;
pub mod remote_panel;

use eframe::egui;

/// Creates a button whose text is truncated with ellipsis when it exceeds available width.
/// Hovering over the button shows the full text in a tooltip.
pub fn ellipsis_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let widget_text = text.into();
    let full_text: String = widget_text.text().to_string();
    ui.add(egui::Button::new(widget_text).truncate())
        .on_hover_text(full_text)
}

/// Creates a conditionally-enabled button with text truncated to ellipsis
/// when it exceeds available width. Hovering shows the full text in a tooltip.
pub fn add_enabled_ellipsis(
    ui: &mut egui::Ui,
    enabled: bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let widget_text = text.into();
    let full_text: String = widget_text.text().to_string();
    ui.add_enabled(enabled, egui::Button::new(widget_text).truncate())
        .on_hover_text(full_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ellipsis_button_returns_response() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_1")).show(ctx, |ui| {
                let response = ellipsis_button(ui, "Test Button");
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        assert!(!response.clicked(), "button should not be clicked in test");
    }

    #[test]
    fn test_ellipsis_button_long_text() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_2")).show(ctx, |ui| {
                let long_text = "A very long button text that would normally overflow the available space";
                let response = ellipsis_button(ui, long_text);
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        assert!(!response.clicked());
        // on_hover_text should have been called - we can't inspect tooltip state directly
        // but the function should not panic with long text
    }

    #[test]
    fn test_add_enabled_ellipsis_disabled() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_3")).show(ctx, |ui| {
                let response = add_enabled_ellipsis(ui, false, "Disabled Button");
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        // When disabled, clicking won't work (test environment)
        assert!(!response.clicked());
    }

    #[test]
    fn test_add_enabled_ellipsis_enabled() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_4")).show(ctx, |ui| {
                let response = add_enabled_ellipsis(ui, true, "Enabled Button");
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        assert!(!response.clicked());
    }

    #[test]
    fn test_ellipsis_button_with_string() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_5")).show(ctx, |ui| {
                let text = String::from("Dynamic String Button");
                let response = ellipsis_button(ui, text);
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        assert!(!response.clicked());
    }

    #[test]
    fn test_ellipsis_button_emoji_short_text() {
        let ctx = egui::Context::default();
        let mut output = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::Area::new(egui::Id::new("test_area_6")).show(ctx, |ui| {
                let response = ellipsis_button(ui, "🔄");
                output = Some(response);
            });
        });
        let response = output.expect("response should be set");
        assert!(!response.clicked());
    }
}

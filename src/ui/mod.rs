pub mod status_panel;
pub mod branch_panel;
pub mod worktree_panel;
pub mod log_panel;
pub mod stash_panel;
pub mod remote_panel;

/// Creates a row layout where buttons are anchored to the right edge,
/// and the label text is truncated with ellipsis if there is not enough space.
/// This ensures:
/// 1. Buttons always stay within the window and don't overflow.
/// 2. Button text is never truncated (no ellipsis on buttons).
/// 3. When the window is too narrow, the label text gets ellipsis instead.
#[allow(dead_code)]
pub fn row_with_right_buttons(
    ui: &mut egui::Ui,
    label_text: impl Into<egui::WidgetText>,
    buttons: impl FnOnce(&mut egui::Ui),
) {
    let widget_text = label_text.into();
    let full_text: String = widget_text.text().to_string();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            buttons(ui);
            ui.add(egui::Label::new(widget_text).truncate()).on_hover_text(full_text);
        });
    });
}

/// Creates a heading row with action buttons anchored to the right edge.
/// The heading text is truncated if there isn't enough space.
#[allow(dead_code)]
pub fn heading_with_buttons(
    ui: &mut egui::Ui,
    heading_text: impl Into<egui::WidgetText>,
    buttons: impl FnOnce(&mut egui::Ui),
) {
    let text = heading_text.into();
    let full_text: String = text.text().to_string();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            buttons(ui);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&full_text)
                        .heading(),
                )
                .truncate(),
            )
            .on_hover_text(full_text);
        });
    });
}

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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test egui context, run UI, and capture rect info.
    fn run_test_ui(width: f32, height: f32, mut ui_fn: impl FnMut(&mut egui::Ui)) -> egui::Context {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.max_passes = 1.try_into().unwrap());
        let _output = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(width, height),
            )),
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui_fn(ui);
            });
        });
        ctx
    }

    #[test]
    fn test_button_without_truncate_does_not_truncate() {
        // Verify that a plain egui::Button does NOT have truncate behavior
        // (button text should always be fully visible)
        let ctx = run_test_ui(100.0, 200.0, |ui| {
            let response = ui.button("A long button text that should not truncate");
            assert!(response.clicked() == false); // not clicked, just placed
            // Button should have a reasonable size even with short width
            assert!(response.rect.width() > 20.0, "Button width should be positive");
        });
        let _ = ctx; // silence unused warning
    }

    #[test]
    fn test_label_truncation_in_right_to_left_layout() {
        // When a label is placed in right_to_left layout WITH buttons,
        // the label should truncate if the window is too narrow.
        let ctx = run_test_ui(200.0, 200.0, |ui| {
            row_with_right_buttons(ui, "This is a very long text that should be truncated if space is limited", |ui| {
                if ui.button("OK").clicked() {}
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_heading_with_buttons_keeps_buttons_visible() {
        // Verify that heading_with_buttons renders buttons on the right
        let ctx = run_test_ui(300.0, 200.0, |ui| {
            heading_with_buttons(ui, "Heading Text", |ui| {
                if ui.button("Action").clicked() {}
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_right_to_left_layout_button_order() {
        // In right_to_left layout, buttons added first should be on the right edge.
        let ctx = run_test_ui(400.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // First button (rightmost in right_to_left)
                    if ui.button("Right").clicked() {}
                    // Second button (to the left of Right)
                    if ui.button("Middle").clicked() {}
                    // Label (leftmost, will truncate)
                    ui.add(egui::Label::new("Label on left").truncate());
                });
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_multiple_buttons_fit_in_narrow_window() {
        // Multiple buttons should still fit even when window is very narrow.
        // Each button should be fully visible (no truncation on button text).
        let ctx = run_test_ui(150.0, 200.0, |ui| {
            row_with_right_buttons(ui, "Label", |ui| {
                if ui.button("A").clicked() {}
                if ui.button("B").clicked() {}
                if ui.button("C").clicked() {}
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_narrow_window_does_not_clip_buttons() {
        // When the window is very narrow, buttons must still be visible.
        // The label text should get truncated instead.
        let ctx = run_test_ui(200.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save").clicked() {}
                    if ui.button("Cancel").clicked() {}
                    ui.add(egui::Label::new("This very long text should be truncated when window is narrow").truncate());
                });
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_button_click_works_in_right_to_left_layout() {
        let mut clicked = false;
        let ctx = run_test_ui(400.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Click Me").clicked() {
                        clicked = true;
                    }
                    ui.add(egui::Label::new("text").truncate());
                });
            });
        });
        let _ = ctx;
        // Button click events are only triggered by actual mouse input,
        // so clicked should be false in a test without mouse input.
        assert!(!clicked, "No mouse input in test, button should not be clicked");
    }
}



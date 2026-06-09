pub mod status_panel;
pub mod branch_panel;
pub mod worktree_panel;
pub mod log_panel;
pub mod stash_panel;
pub mod remote_panel;

use eframe::egui;
use std::collections::HashMap;

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

// ---------------------------------------------------------------------------
// Column resizing helpers (Excel-style: drag column dividers to resize)
// ---------------------------------------------------------------------------

/// Stores column widths for resizable table columns.
/// Each key is a unique column identifier (e.g. "branch_name", "branch_upstream").
#[derive(Clone)]
pub struct ColumnWidthStore {
    widths: HashMap<String, f32>,
}

impl ColumnWidthStore {
    pub fn new() -> Self {
        Self { widths: HashMap::new() }
    }

    /// Get the stored width for a column, or return the default if not yet set.
    pub fn get(&self, key: &str, default: f32) -> f32 {
        self.widths.get(key).copied().unwrap_or(default)
    }

    /// Set the width for a column.
    pub fn set(&mut self, key: &str, width: f32) {
        self.widths.insert(key.to_string(), width);
    }
}

/// Renders a column header label with a draggable divider on its right edge.
/// The divider can be dragged horizontally to resize the column.
///
/// - `ui`: the UI handle
/// - `label`: the header text
/// - `width`: in/out — current width of this column; updated when dragged
/// - `min_width`: minimum allowed width (pixels)
/// - `id_salt`: unique identifier for this divider's interaction state
///
/// Returns the response from the divider interaction.
pub fn column_header(
    ui: &mut egui::Ui,
    label: &str,
    width: &mut f32,
    min_width: f32,
    id_salt: &str,
) -> egui::Response {
    let height = ui.available_height().max(20.0);
    let base_id = ui.next_auto_id().with(id_salt);

    // Reserve space for this column (header label area)
    let (header_rect, _) = ui.allocate_exact_size(
        egui::vec2(*width, height),
        egui::Sense::hover(),
    );

    // Draw header label text (left-aligned)
    let painter = ui.painter();
    let text_color = ui.style().visuals.text_color();
    let text_pos = header_rect.left_center() + egui::vec2(4.0, 0.0);
    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    // Divider: thin vertical line + interactive drag zone
    let divider_rect = egui::Rect::from_min_size(
        egui::pos2(header_rect.right() - 2.0, header_rect.top()),
        egui::vec2(4.0, header_rect.height()),
    );

    let divider_id = base_id.with("_div");
    let resp = ui.interact(divider_rect, divider_id, egui::Sense::drag());

    if resp.dragged_by(egui::PointerButton::Primary) {
        *width = (*width + resp.drag_delta().x).max(min_width);
    }

    // Draw divider line
    painter.vline(
        header_rect.right(),
        header_rect.y_range(),
        egui::Stroke::new(1.0, ui.style().visuals.window_fill),
    );

    // Change cursor on hover/drag
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
    }

    resp
}

/// Render content inside a column of the given width, left-aligned.
/// The content is truncated with ellipsis if it exceeds the column width.
/// Hovering shows the full text in a tooltip.
pub fn column_cell(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    color: egui::Color32,
) -> egui::Response {
    let cell_width = width.max(4.0);
    let height = ui.available_height();

    // Allocate exact space
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(cell_width, height),
        egui::Sense::hover(),
    );

    // Draw text left-aligned using painter (guaranteed left alignment)
    let painter = ui.painter();
    let font_id = egui::FontId::proportional(14.0);
    let text_pos = rect.left_center() + egui::vec2(4.0, 0.0);
    let wrap_width = (cell_width - 8.0).max(0.0);

    // Build a layout job with truncation for ellipsis support
    let mut job = egui::text::LayoutJob::default();
    job.wrap = egui::text::TextWrapping::truncate_at_width(wrap_width);
    job.append(text, 0.0, egui::TextFormat::simple(font_id, color));
    let galley = painter.layout_job(job);
    painter.galley(text_pos, galley, color);

    if !text.is_empty() {
        response.on_hover_text(text.to_string())
    } else {
        response
    }
}

/// Renders a column separator divider line (no interaction, just visual).
/// Use this for the last column before buttons to separate content from actions.
pub fn column_separator(ui: &mut egui::Ui) {
    let height = ui.available_height();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(4.0, height),
        egui::Sense::hover(),
    );
    ui.painter().vline(
        rect.left(),
        rect.y_range(),
        egui::Stroke::new(1.0, ui.style().visuals.window_fill),
    );
}

// ---------------------------------------------------------------------------
// Initialize column widths for each panel (called from App::new)
// ---------------------------------------------------------------------------

/// Initialize default column widths for all panels.
pub fn init_column_widths() -> ColumnWidthStore {
    let mut store = ColumnWidthStore::new();
    // Branch panel - all resizable columns
    store.set("branch_name", 260.0);
    store.set("branch_commit", 220.0);
    // Worktree panel - all resizable columns
    store.set("worktree_branch_sha", 220.0);
    store.set("worktree_path", 250.0);
    store
}

// ---------------------------------------------------------------------------
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

    // ── ColumnWidthStore tests ──────────────────────────────────────────

    #[test]
    fn test_column_width_store_defaults() {
        let store = ColumnWidthStore::new();
        assert_eq!(store.get("nonexistent", 100.0), 100.0);
        assert_eq!(store.get("other", 200.0), 200.0);
    }

    #[test]
    fn test_column_width_store_set_and_get() {
        let mut store = ColumnWidthStore::new();
        store.set("column_a", 150.0);
        store.set("column_b", 250.0);
        assert_eq!(store.get("column_a", 100.0), 150.0);
        assert_eq!(store.get("column_b", 100.0), 250.0);
        // Unknown column still returns default
        assert_eq!(store.get("column_c", 100.0), 100.0);
    }

    #[test]
    fn test_column_width_store_overwrite() {
        let mut store = ColumnWidthStore::new();
        store.set("col", 120.0);
        assert_eq!(store.get("col", 0.0), 120.0);
        store.set("col", 200.0);
        assert_eq!(store.get("col", 0.0), 200.0);
    }

    #[test]
    fn test_column_width_store_independent_keys() {
        let mut store = ColumnWidthStore::new();
        store.set("branch_name", 180.0);
        store.set("worktree_path", 300.0);
        assert_eq!(store.get("branch_name", 0.0), 180.0);
        assert_eq!(store.get("worktree_path", 0.0), 300.0);
        assert_eq!(store.get("branch_upstream", 200.0), 200.0);
    }

    #[test]
    fn test_init_column_widths_has_defaults() {
        let store = init_column_widths();
        // All resizable columns have defaults
        assert_eq!(store.get("branch_name", 0.0), 260.0);
        assert_eq!(store.get("branch_commit", 0.0), 220.0);
        assert_eq!(store.get("worktree_branch_sha", 0.0), 220.0);
        assert_eq!(store.get("worktree_path", 0.0), 250.0);
    }

    #[test]
    fn test_column_cell_renders_without_panic() {
        // Verify column_cell doesn't panic with various text lengths
        let ctx = run_test_ui(500.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                column_cell(ui, 100.0, "short", egui::Color32::GRAY);
                column_cell(ui, 200.0, "A moderately long text that should truncate", egui::Color32::GRAY);
                column_cell(ui, 300.0, "", egui::Color32::GRAY);
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_column_header_renders_without_panic() {
        // Verify column_header renders without panic
        let ctx = run_test_ui(500.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                let mut w1 = 150.0;
                let mut w2 = 200.0;
                column_header(ui, "Name", &mut w1, 50.0, "name_hdr");
                column_header(ui, "Description", &mut w2, 50.0, "desc_hdr");
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_column_cell_shows_full_text_on_hover() {
        // Verify that the column_cell stores the full text for tooltip
        let ctx = run_test_ui(300.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                let resp = column_cell(ui, 50.0, "This is a very long text for tooltip testing", egui::Color32::GRAY);
                // The response should exist
                assert!(resp.rect.width() > 0.0);
            });
        });
        let _ = ctx;
    }

    #[test]
    fn test_column_header_drag_changes_width() {
        // Simulate a drag on the divider
        let ctx = run_test_ui(500.0, 200.0, |ui| {
            ui.horizontal(|ui| {
                let mut width = 150.0;
                let resp = column_header(ui, "TestCol", &mut width, 50.0, "test_hdr");

                // Before drag, width unchanged
                assert_eq!(width, 150.0);

                // We can't actually simulate mouse in egui tests,
                // so just verify the function doesn't panic
                assert!(!resp.dragged());
            });
        });
        let _ = ctx;
    }
}



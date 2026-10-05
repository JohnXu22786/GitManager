#[path = "support/egui_harness.rs"]
mod egui_harness;

use egui_harness::EguiHarness;

fn button(ctx: &egui::Context) -> egui::Response {
    egui::CentralPanel::default()
        .show(ctx, |ui| ui.button("Apply"))
        .inner
}

fn editor(ctx: &egui::Context, text: &mut String) -> egui::Response {
    egui::CentralPanel::default()
        .show(ctx, |ui| ui.text_edit_singleline(text))
        .inner
}

#[test]
fn pointer_frames_preserve_holds_and_do_not_replay_clicks() {
    let mut harness = EguiHarness::new(egui::vec2(640.0, 480.0));
    let position = harness.frame(button).rect.center();

    for _ in 0..2 {
        harness.press_at(position);
        let pressed = harness.frame(button);
        assert!(pressed.is_pointer_button_down_on());
        assert!(!pressed.clicked());
        let held = harness.frame(button);
        assert!(held.is_pointer_button_down_on());
        assert!(!held.clicked());

        harness.release_at(position);
        let released = harness.frame(button);
        assert!(released.clicked());
        assert!(!released.is_pointer_button_down_on());
        assert!(!harness.frame(button).clicked());
    }
}

#[test]
fn keyboard_focus_routes_unicode_text_and_shift_tab() {
    let mut harness = EguiHarness::new(egui::vec2(640.0, 480.0));
    let mut first = String::new();
    let mut second = String::new();
    let mut fields = |ctx: &egui::Context| {
        egui::CentralPanel::default()
            .show(ctx, |ui| {
                (
                    ui.text_edit_singleline(&mut first),
                    ui.text_edit_singleline(&mut second),
                )
            })
            .inner
    };
    let position = harness.frame(&mut fields).0.rect.center();
    harness.press_at(position);
    harness.frame(&mut fields);
    harness.release_at(position);
    assert!(harness.frame(&mut fields).0.has_focus());

    let long_text = "中文工作记录🦀".repeat(40);
    harness.text(&long_text);
    harness.frame(&mut fields);
    harness.frame(&mut fields);
    harness.key(egui::Key::Tab, true, egui::Modifiers::NONE);
    harness.frame(&mut fields);
    harness.key(egui::Key::Tab, false, egui::Modifiers::NONE);
    assert!(harness.frame(&mut fields).1.has_focus());
    harness.text("下一笔");
    harness.frame(&mut fields);

    harness.key(egui::Key::Tab, true, egui::Modifiers::SHIFT);
    harness.frame(&mut fields);
    harness.key(egui::Key::Tab, false, egui::Modifiers::NONE);
    assert!(harness.frame(&mut fields).0.has_focus());
    assert_eq!(first, long_text);
    assert_eq!(second, "下一笔");
}

#[test]
fn keyboard_repeat_and_release_edit_only_the_focused_widget() {
    let mut harness = EguiHarness::new(egui::vec2(640.0, 480.0));
    let mut text = String::new();
    let position = harness.frame(|ctx| editor(ctx, &mut text)).rect.center();
    harness.press_at(position);
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.release_at(position);
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.text("甲乙丙");
    harness.frame(|ctx| editor(ctx, &mut text));

    harness.key(egui::Key::Backspace, true, egui::Modifiers::NONE);
    harness.frame(|ctx| editor(ctx, &mut text));
    assert_eq!(text, "甲乙");
    harness.frame(|ctx| editor(ctx, &mut text));
    assert_eq!(text, "甲乙", "held keys do not synthesize OS repeat events");
    harness.key(egui::Key::Backspace, true, egui::Modifiers::NONE);
    harness.frame(|ctx| editor(ctx, &mut text));
    assert_eq!(text, "甲");
    harness.key(egui::Key::Backspace, false, egui::Modifiers::NONE);
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.frame(|ctx| editor(ctx, &mut text));
    assert_eq!(text, "甲");
}

#[test]
fn resize_and_window_focus_preserve_text_and_widget_identity() {
    let mut harness = EguiHarness::new(egui::vec2(800.0, 600.0));
    let mut text = String::new();
    let initial = harness.frame(|ctx| editor(ctx, &mut text));
    harness.press_at(initial.rect.center());
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.release_at(initial.rect.center());
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.text("保留未保存内容");
    harness.frame(|ctx| editor(ctx, &mut text));

    harness.resize(egui::vec2(240.0, 360.0));
    harness.set_focused(false);
    let blurred = harness.frame(|ctx| {
        ctx.input(|input| {
            assert!(!input.focused);
            assert_eq!(input.raw.viewport().focused, Some(false));
            assert_eq!(input.raw.viewport().inner_rect, Some(input.screen_rect()));
            assert_eq!(input.screen_rect().size(), egui::vec2(240.0, 360.0));
        });
        editor(ctx, &mut text)
    });
    assert_eq!(blurred.id, initial.id);
    assert!(blurred.rect.width() < initial.rect.width());
    assert!(!blurred.has_focus());
    harness.set_focused(true);
    let focused = harness.frame(|ctx| editor(ctx, &mut text));
    assert!(focused.has_focus());
    harness.text("继续");
    harness.frame(|ctx| editor(ctx, &mut text));
    assert_eq!(text, "保留未保存内容继续");
}

#[test]
fn deterministic_frames_drain_text_even_when_egui_repeats_layout() {
    let mut harness = EguiHarness::new(egui::vec2(640.0, 480.0));
    let mut text = String::new();
    let mut times = Vec::new();
    let initial = harness.frame(|ctx| {
        times.push(ctx.input(|input| input.time));
        editor(ctx, &mut text)
    });
    harness.press_at(initial.rect.center());
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.release_at(initial.rect.center());
    harness.frame(|ctx| editor(ctx, &mut text));
    harness.text("once");
    let mut passes = 0;
    harness.frame(|ctx| {
        times.push(ctx.input(|input| input.time));
        let response = editor(ctx, &mut text);
        passes += 1;
        if passes == 1 {
            ctx.request_discard("exercise an extra layout pass");
        }
        response
    });
    assert_eq!(passes, 2);
    assert_eq!(text, "once");
    harness.frame(|ctx| {
        times.push(ctx.input(|input| input.time));
        editor(ctx, &mut text)
    });
    assert_eq!(text, "once");
    assert_eq!(times, vec![0.0, 3.0 / 60.0, 3.0 / 60.0, 4.0 / 60.0]);
}

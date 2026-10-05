//! Headless input driver shared by app unit tests and path-importing integration tests.
//!
//! Render once to obtain real widget rectangles, then queue input before each frame.
//! Press and release belong in separate frames; idle frames retain egui's held state.
//! This drives one root viewport at 60 Hz in logical points, without a renderer or OS.
//! It does not emulate native dialogs, clipboard, IME, automatic key repeat, or OS
//! focus-loss key/pointer cleanup. Tests must provide those input events explicitly.
//! Passing here is not desktop, font rendering, or accessibility verification.

use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

pub struct EguiHarness {
    ctx: Context,
    screen_rect: Rect,
    focused: bool,
    modifiers: Modifiers,
    events: Vec<Event>,
    frame_index: u64,
}

// This path-imported helper has different consumers in each test target.
#[allow(dead_code)]
impl EguiHarness {
    pub fn new(size: Vec2) -> Self {
        Self {
            ctx: Context::default(),
            screen_rect: Rect::from_min_size(Pos2::ZERO, size),
            focused: true,
            modifiers: Modifiers::NONE,
            events: Vec::new(),
            frame_index: 0,
        }
    }

    pub fn resize(&mut self, size: Vec2) {
        self.screen_rect = Rect::from_min_size(Pos2::ZERO, size);
    }

    pub fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.events.push(Event::WindowFocused(focused));
            self.focused = focused;
        }
    }

    pub fn move_to(&mut self, position: Pos2) {
        self.events.push(Event::PointerMoved(position));
    }

    pub fn press_at(&mut self, position: Pos2) {
        self.pointer_button(position, true);
    }

    pub fn release_at(&mut self, position: Pos2) {
        self.pointer_button(position, false);
    }

    fn pointer_button(&mut self, position: Pos2, pressed: bool) {
        self.move_to(position);
        self.events.push(Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        });
    }

    /// Send an explicit key transition; another press while held is an egui repeat.
    /// Modifiers remain set for later frames until the next call changes them.
    pub fn key(&mut self, key: Key, pressed: bool, modifiers: Modifiers) {
        self.modifiers = modifiers;
        self.events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false, // egui derives repeat from the preceding key state.
            modifiers,
        });
    }

    /// Queue committed text, not native keyboard layout or IME composition.
    pub fn text(&mut self, text: &str) {
        self.events.push(Event::Text(text.to_owned()));
    }

    /// Run the supplied real UI and return its last pass's response/value.
    /// Like `Context::run`, the closure may run more than once for egui layout.
    /// Queued events are consumed once; each call advances time by 1/60 second.
    pub fn frame<T>(&mut self, mut ui: impl FnMut(&Context) -> T) -> T {
        let mut input = RawInput {
            screen_rect: Some(self.screen_rect),
            time: Some(self.frame_index as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            focused: self.focused,
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.inner_rect = Some(self.screen_rect);
        viewport.native_pixels_per_point = Some(1.0);
        viewport.focused = Some(self.focused);
        self.frame_index += 1;

        let mut result = None;
        let _ = self.ctx.run(input, |ctx| result = Some(ui(ctx)));
        result.expect("egui must run at least one UI pass")
    }
}

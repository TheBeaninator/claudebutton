//! Touch-surface stroke classifier. Some gadgets (e.g. the JX-05 ring) fire canned
//! swipe strokes on an emulated digitizer instead of sending key codes. Feed raw
//! events; get back a discrete gesture at touch-up.

use std::time::Instant;

use evdev::{AbsoluteAxisType, EventType, InputEvent, Key};

/// Gesture name matches the lowercase keys used in a profile's `map`.
pub fn name(g: Gesture) -> &'static str {
    match g {
        Gesture::Up => "up",
        Gesture::Down => "down",
        Gesture::Left => "left",
        Gesture::Right => "right",
        Gesture::Tap => "tap",
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Gesture {
    Up,
    Down,
    Left,
    Right,
    Tap,
}

pub struct GestureEngine {
    tap_dist: f64,
    active: bool,
    x0: Option<i32>,
    y0: Option<i32>,
    x: Option<i32>,
    y: Option<i32>,
    t0: Instant,
}

impl GestureEngine {
    pub fn new(tap_dist: f64) -> Self {
        Self {
            tap_dist,
            active: false,
            x0: None,
            y0: None,
            x: None,
            y: None,
            t0: Instant::now(),
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.x0 = None;
        self.y0 = None;
        self.x = None;
        self.y = None;
    }

    /// Feed one event; returns Some(gesture) when a stroke completes.
    pub fn feed(&mut self, ev: &InputEvent) -> Option<Gesture> {
        match ev.event_type() {
            EventType::KEY if ev.code() == Key::BTN_TOUCH.code() => {
                if ev.value() == 1 {
                    self.reset();
                    self.active = true;
                    self.t0 = Instant::now();
                    None
                } else if ev.value() == 0 {
                    self.end()
                } else {
                    None
                }
            }
            EventType::ABSOLUTE if self.active => {
                let c = ev.code();
                if c == AbsoluteAxisType::ABS_X.0 || c == AbsoluteAxisType::ABS_MT_POSITION_X.0 {
                    self.x = Some(ev.value());
                    if self.x0.is_none() {
                        self.x0 = Some(ev.value());
                    }
                } else if c == AbsoluteAxisType::ABS_Y.0
                    || c == AbsoluteAxisType::ABS_MT_POSITION_Y.0
                {
                    self.y = Some(ev.value());
                    if self.y0.is_none() {
                        self.y0 = Some(ev.value());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn end(&mut self) -> Option<Gesture> {
        if !self.active {
            return None;
        }
        self.active = false;
        let dx = match (self.x, self.x0) {
            (Some(a), Some(b)) => (a - b) as f64,
            _ => 0.0,
        };
        let dy = match (self.y, self.y0) {
            (Some(a), Some(b)) => (a - b) as f64,
            _ => 0.0,
        };
        let dist = (dx * dx + dy * dy).sqrt();
        Some(if dist < self.tap_dist {
            Gesture::Tap
        } else if dy.abs() >= dx.abs() {
            if dy > 0.0 {
                Gesture::Up
            } else {
                Gesture::Down
            }
        } else if dx > 0.0 {
            Gesture::Right
        } else {
            Gesture::Left
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(down: bool) -> InputEvent {
        InputEvent::new(
            EventType::KEY,
            Key::BTN_TOUCH.code(),
            if down { 1 } else { 0 },
        )
    }
    fn abs_y(v: i32) -> InputEvent {
        InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, v)
    }
    fn abs_x(v: i32) -> InputEvent {
        InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, v)
    }

    fn stroke(events: &[InputEvent]) -> Option<Gesture> {
        let mut e = GestureEngine::new(400.0);
        let mut last = None;
        for ev in events {
            if let Some(g) = e.feed(ev) {
                last = Some(g);
            }
        }
        last
    }

    #[test]
    fn classifies_vertical_and_horizontal() {
        assert!(matches!(
            stroke(&[touch(true), abs_y(100), abs_y(2000), touch(false)]),
            Some(Gesture::Up)
        ));
        assert!(matches!(
            stroke(&[touch(true), abs_y(2000), abs_y(100), touch(false)]),
            Some(Gesture::Down)
        ));
        assert!(matches!(
            stroke(&[touch(true), abs_x(100), abs_x(2000), touch(false)]),
            Some(Gesture::Right)
        ));
        assert!(matches!(
            stroke(&[touch(true), abs_x(2000), abs_x(100), touch(false)]),
            Some(Gesture::Left)
        ));
    }

    #[test]
    fn zero_movement_is_a_tap() {
        assert!(matches!(
            stroke(&[touch(true), touch(false)]),
            Some(Gesture::Tap)
        ));
    }
}
